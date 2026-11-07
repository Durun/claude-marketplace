//! 別プロセスのホストの持ち方。ページごとにホストを 1 つ読み込んだまま置き、同じページの次のフレームは
//! `RenderFrame` だけで済ませる。ホストの数は上限までで、溢れたら最後に使ったのが古いホストにページを読み直させる。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mm_host_api::{HostLauncher, HostProcess, HostRequest, HostResponse, HostSpec, SharedFrame};
use mm_types::{GpuAdapterId, MmError, Result};

use crate::page::FrameRequest;

/// 同時に起こしておくホストの数。1 つの CEF は数百 MB を使うので、並べて使う部品の数（2〜3）に留める。
const MAX_HOSTS: usize = 3;
/// 1 フレームの期限。ページの描画と共有テクスチャの受け渡しを含む。
const RENDER_TIMEOUT: Duration = Duration::from_secs(5);
/// ページの読み込みの期限。
const PREPARE_TIMEOUT: Duration = Duration::from_secs(15);

/// ホストに頼む受け取り方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// CPU の画像（RGBA f32、premultiplied、線形）。
    Cpu,
    /// このアダプターの上の共有テクスチャ。`device` はレンダラーのデバイスの世代で、作り直すたびに変わる。
    Shared { adapter: GpuAdapterId, device: u64 },
}

struct Slot {
    page: u64,
    delivery: Delivery,
    host: Box<dyn HostProcess>,
}

/// ホストの一式。
pub struct HostPages {
    launcher: Arc<dyn HostLauncher>,
    executable: PathBuf,
    /// 新しく使った順。
    slots: Vec<Slot>,
}

impl HostPages {
    /// `executable` を `launcher` で起こす。起こすのは最初のフレームを頼まれたとき。
    pub fn new(launcher: Arc<dyn HostLauncher>, executable: PathBuf) -> Self {
        Self {
            launcher,
            executable,
            slots: Vec::new(),
        }
    }

    fn spec(&self, delivery: &Delivery) -> HostSpec {
        let (gpu, client_pid) = match delivery {
            Delivery::Cpu => (None, None),
            Delivery::Shared { adapter, .. } => (Some(adapter.clone()), Some(std::process::id())),
        };
        HostSpec {
            executable: self.executable.clone(),
            args: Vec::new(),
            default_timeout: RENDER_TIMEOUT,
            gpu,
            client_pid,
        }
    }

    /// 1 フレームを描く。ホストがクラッシュしていたら 1 回だけ起こし直してやり直す。
    pub fn render(&mut self, req: &FrameRequest<'_>, delivery: &Delivery) -> Result<SharedFrame> {
        let _span = tracing::info_span!("nodes_cef.host_frame").entered();
        self.retire_other_devices(delivery);
        match self.try_render(req, delivery) {
            Err(MmError::HostCrashed(why)) => {
                tracing::warn!(%why, "web.cef のホストを起こし直す");
                self.slots.retain(|s| s.host.is_alive());
                self.try_render(req, delivery)
            }
            other => other,
        }
    }

    /// 共有テクスチャで頼むとき、ほかのデバイスの世代に向けて起こしたホストを止める。
    /// デバイスを作り直すと、古いホストは古いアダプター（か消えたデバイス）の上で描き続けるため、
    /// 次のフレームで新しいアダプターのホストを起こし直す。
    fn retire_other_devices(&mut self, delivery: &Delivery) {
        if matches!(delivery, Delivery::Shared { .. }) {
            self.slots
                .retain(|s| s.delivery == Delivery::Cpu || s.delivery == *delivery);
        }
    }

    fn try_render(&mut self, req: &FrameRequest<'_>, delivery: &Delivery) -> Result<SharedFrame> {
        let slot = self.slot_for(req, delivery)?;
        let request = HostRequest::RenderFrame {
            time: req.time,
            size: req.size,
            params: req.page.frame_params(req.scale),
        };
        match slot.host.request(&request, RENDER_TIMEOUT)? {
            HostResponse::Frame(frame) => Ok(frame),
            HostResponse::Error { message } => Err(MmError::Internal(format!(
                "web.cef のページを描けない: {message}"
            ))),
            other => Err(MmError::Internal(format!(
                "RenderFrame に Frame でない応答: {other:?}"
            ))),
        }
    }

    /// そのページを読み込んだホスト。無ければ起こすか、古いホストに読み直させる。先頭に置いて返す。
    fn slot_for(&mut self, req: &FrameRequest<'_>, delivery: &Delivery) -> Result<&mut Slot> {
        let hit = self
            .slots
            .iter()
            .position(|s| s.page == req.page.hash && s.delivery == *delivery);
        let slot = match hit {
            Some(i) => self.slots.remove(i),
            None => self.load(req, delivery)?,
        };
        self.slots.insert(0, slot);
        Ok(&mut self.slots[0])
    }

    fn load(&mut self, req: &FrameRequest<'_>, delivery: &Delivery) -> Result<Slot> {
        let reuse = (self.slots.len() >= MAX_HOSTS)
            .then(|| self.slots.iter().rposition(|s| s.delivery == *delivery))
            .flatten();
        let mut host = match reuse {
            Some(i) => self.slots.remove(i).host,
            None => {
                if self.slots.len() >= MAX_HOSTS {
                    self.slots.pop();
                }
                self.launcher.spawn(&self.spec(delivery))?
            }
        };
        let prepare = HostRequest::Prepare {
            params: serde_json::json!({ "html": req.page.html }),
        };
        match host.request(&prepare, PREPARE_TIMEOUT)? {
            HostResponse::Prepared => Ok(Slot {
                page: req.page.hash,
                delivery: delivery.clone(),
                host,
            }),
            other => Err(MmError::Internal(format!(
                "web.cef のページを読み込めない: {other:?}"
            ))),
        }
    }
}

impl std::fmt::Debug for HostPages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostPages")
            .field("launcher", &self.launcher.id())
            .field("executable", &self.executable)
            .field("live", &self.slots.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use mm_types::{ParamValue, Params, RationalTime, Size};

    use super::*;
    use crate::page::{Page, HTML_PARAM};

    /// 起こしたホストの `gpu` と、生きているホストの数を数えるランチャー。
    #[derive(Default)]
    struct Counting {
        spawned: Mutex<Vec<Option<GpuAdapterId>>>,
        live: Arc<AtomicUsize>,
    }

    struct Fake {
        live: Arc<AtomicUsize>,
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl HostProcess for Fake {
        fn is_alive(&self) -> bool {
            true
        }
        fn pid(&self) -> Option<u32> {
            None
        }
        fn request(&mut self, req: &HostRequest, _timeout: Duration) -> Result<HostResponse> {
            Ok(match req {
                HostRequest::Prepare { .. } => HostResponse::Prepared,
                _ => HostResponse::Frame(SharedFrame::Cpu {
                    size: Size::new(1, 1),
                    data: vec![0.0; 4],
                }),
            })
        }
        fn kill(&mut self) -> Result<()> {
            Ok(())
        }
        fn restart(&mut self) -> Result<()> {
            Ok(())
        }
        fn open_handles(&self) -> usize {
            0
        }
    }

    impl HostLauncher for Counting {
        fn id(&self) -> &'static str {
            "counting"
        }
        fn spawn(&self, spec: &HostSpec) -> Result<Box<dyn HostProcess>> {
            self.spawned.lock().unwrap().push(spec.gpu.clone());
            self.live.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Fake {
                live: self.live.clone(),
            }))
        }
    }

    fn adapter(name: &str) -> GpuAdapterId {
        GpuAdapterId {
            vendor: 0x10de,
            device: 1,
            name: name.into(),
            luid: Some(1),
        }
    }

    #[test]
    fn デバイスを作り直したらホストを新しいアダプターで起こし直す() {
        let launcher = Arc::new(Counting::default());
        let mut hosts = HostPages::new(launcher.clone(), "fake".into());
        let mut params = Params::new();
        params.set(HTML_PARAM, ParamValue::Text("<p>x</p>".into()));
        let page = Page::read(&params).unwrap();
        let req = FrameRequest {
            page: &page,
            time: RationalTime::ZERO,
            size: Size::new(1, 1),
            scale: 1.0,
        };
        let old = Delivery::Shared {
            adapter: adapter("old"),
            device: 1,
        };
        hosts.render(&req, &old).unwrap();
        hosts.render(&req, &Delivery::Cpu).unwrap();
        hosts.render(&req, &old).unwrap();
        assert_eq!(
            launcher.spawned.lock().unwrap().len(),
            2,
            "同じデバイスなら使い回す"
        );

        let fresh = Delivery::Shared {
            adapter: adapter("new"),
            device: 2,
        };
        hosts.render(&req, &fresh).unwrap();
        let spawned = launcher.spawned.lock().unwrap().clone();
        assert_eq!(spawned.last(), Some(&Some(adapter("new"))));
        assert_eq!(
            launcher.live.load(Ordering::SeqCst),
            2,
            "古いデバイスのホストは止め、CPU の経路のホストは残す"
        );
    }
}
