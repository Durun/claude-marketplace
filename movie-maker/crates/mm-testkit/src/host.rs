//! 別プロセスのホストの見本と契約テスト。見本はプロセスを起こさず、同じプロセスの中で応答する。
//!
//! 時間切れの検査のため、`RenderFrame` の `params` に `{"sample_delay_ms": N}` があれば、
//! N が期限を超えるときに `Timeout` を返す（実際には待たない）。

use std::sync::Arc;
use std::time::Duration;

use mm_host_api::{
    HostLauncher, HostProcess, HostRequest, HostResponse, HostSpec, SharedFrame, PROTOCOL_VERSION,
};
use mm_types::{MmError, Result, Rgba, RgbaImage, Size};

/// 見本が見る「遅さ」のパラメータの名前。
pub const DELAY_PARAM: &str = "sample_delay_ms";

/// 見本のホストを起こす口。
#[derive(Debug, Clone, Default)]
pub struct SampleLauncher;

impl HostLauncher for SampleLauncher {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn spawn(&self, _spec: &HostSpec) -> Result<Box<dyn HostProcess>> {
        Ok(Box::new(SampleHost {
            alive: true,
            prepared: false,
            generation: 1,
        }))
    }
}

/// 見本のホスト。
#[derive(Debug)]
pub struct SampleHost {
    alive: bool,
    prepared: bool,
    generation: u32,
}

impl SampleHost {
    fn render(size: Size, params: &serde_json::Value, timeout: Duration) -> Result<HostResponse> {
        let delay = params
            .get(DELAY_PARAM)
            .and_then(serde_json::Value::as_u64)
            .map(Duration::from_millis)
            .unwrap_or_default();
        if delay > timeout {
            return Err(MmError::Timeout {
                what: "host.render_frame".into(),
                limit: timeout,
            });
        }
        let image = RgbaImage::filled(size, Rgba::new(0.0, 0.5, 0.0, 1.0));
        Ok(HostResponse::Frame(SharedFrame::from_image(&image)))
    }
}

impl HostProcess for SampleHost {
    fn is_alive(&self) -> bool {
        self.alive
    }

    fn pid(&self) -> Option<u32> {
        self.alive.then_some(self.generation)
    }

    fn request(&mut self, req: &HostRequest, timeout: Duration) -> Result<HostResponse> {
        if !self.alive {
            return Err(MmError::HostCrashed("見本のホストは落ちている".into()));
        }
        match req {
            HostRequest::Ping => Ok(HostResponse::Pong {
                protocol_version: PROTOCOL_VERSION,
            }),
            HostRequest::Prepare { .. } => {
                self.prepared = true;
                Ok(HostResponse::Prepared)
            }
            HostRequest::RenderFrame { size, params, .. } => {
                if !self.prepared {
                    return Ok(HostResponse::Error {
                        message: "Prepare の前に RenderFrame".into(),
                    });
                }
                Self::render(*size, params, timeout)
            }
            HostRequest::Shutdown => {
                self.alive = false;
                Ok(HostResponse::Pong {
                    protocol_version: PROTOCOL_VERSION,
                })
            }
        }
    }

    fn kill(&mut self) -> Result<()> {
        self.alive = false;
        Ok(())
    }

    fn restart(&mut self) -> Result<()> {
        self.alive = true;
        self.prepared = false;
        self.generation += 1;
        Ok(())
    }

    fn open_handles(&self) -> usize {
        0
    }
}

/// 契約テストに渡すホストの情報。
#[derive(Debug, Clone)]
pub struct HostFixture {
    pub spec: HostSpec,
    /// `Prepare` に渡す値。
    pub prepare_params: serde_json::Value,
    /// 期限を超える `RenderFrame` の `params`。
    pub slow_params: serde_json::Value,
}

impl HostFixture {
    /// 見本のホスト向け。
    pub fn sample() -> Self {
        Self {
            spec: HostSpec {
                executable: "mm-host-sample".into(),
                args: Vec::new(),
                default_timeout: Duration::from_millis(100),
                gpu: None,
                client_pid: None,
            },
            prepare_params: serde_json::json!({}),
            slow_params: serde_json::json!({ DELAY_PARAM: 10_000 }),
        }
    }
}

/// ホストの契約。
pub fn contract(make: impl Fn() -> Arc<dyn HostLauncher>, fx: &HostFixture) {
    let launcher = make();
    let mut host = launcher.spawn(&fx.spec).expect("起動");
    let timeout = fx.spec.default_timeout;
    assert!(host.is_alive(), "起動直後は生きている");
    assert!(
        matches!(
            host.request(&HostRequest::Ping, timeout),
            Ok(HostResponse::Pong { .. })
        ),
        "Ping に Pong"
    );
    let prepare = HostRequest::Prepare {
        params: fx.prepare_params.clone(),
    };
    assert!(matches!(
        host.request(&prepare, timeout),
        Ok(HostResponse::Prepared)
    ));

    let size = Size::new(32, 16);
    let render = HostRequest::RenderFrame {
        time: mm_types::RationalTime::ZERO,
        size,
        params: serde_json::json!({}),
    };
    match host.request(&render, timeout).expect("描ける") {
        HostResponse::Frame(frame) => assert_eq!(frame.size(), size, "頼んだ大きさ"),
        other => panic!("Frame のはずが {other:?}"),
    }

    check_timeout_and_restart(host.as_mut(), fx, size);
}

fn check_timeout_and_restart(host: &mut dyn HostProcess, fx: &HostFixture, size: Size) {
    let timeout = fx.spec.default_timeout;
    let slow = HostRequest::RenderFrame {
        time: mm_types::RationalTime::ZERO,
        size,
        params: fx.slow_params.clone(),
    };
    assert!(
        matches!(host.request(&slow, timeout), Err(MmError::Timeout { .. })),
        "期限を過ぎたら Timeout"
    );
    assert!(host.is_alive(), "時間切れの後もホストは使える");
    assert_eq!(host.open_handles(), 0, "ハンドルが漏れていない");

    host.kill().expect("kill");
    assert!(!host.is_alive(), "kill の後は死んでいる");
    host.restart().expect("restart");
    assert!(host.is_alive(), "restart の後は生きている");
    assert!(
        matches!(
            host.request(&HostRequest::Ping, timeout),
            Ok(HostResponse::Pong { .. })
        ),
        "再起動後も Ping が通る"
    );
}
