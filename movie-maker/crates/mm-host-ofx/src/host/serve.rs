//! `mm-host-ofx serve`: 標準入力の `HostRequest` に 1 行ずつ `HostResponse` で答える。
//! 要求は 1 つずつ順に処理する（期限を過ぎた応答の読み捨てはランチャーの側が数える）。

use std::io::{BufRead, Write};
use std::mem::ManuallyDrop;

use mm_host_api::{HostRequest, HostResponse, SharedFrame, PROTOCOL_VERSION};
use mm_host_ofx::{PrepareParams, RenderParams};
use mm_types::{RationalTime, Size};

use super::names::FX_SUPPORTED_DEPTHS;
use super::pixels::Depth;
use super::plugin::{Bundle, Described, Plugin};
use super::render::Instance;

/// 読み込んだ 1 つのプラグインとインスタンス。捨てるときは DestroyInstance、Unload の順に呼ぶ。
struct Session {
    instance: Option<Instance>,
    plugin: Plugin,
    described: Described,
    /// 共有ライブラリは解放しない（`plugin.rs` の冒頭）。
    _bundle: ManuallyDrop<Bundle>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.instance.take();
        self.plugin.unload();
    }
}

#[derive(Default)]
struct Server {
    session: Option<Session>,
}

/// 標準入力が閉じるか `Shutdown` が来るまで答え続ける。応答は `out` に書く。
pub fn run(out: &mut impl Write) -> Result<(), String> {
    let mut server = Server::default();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| format!("要求を読めない: {e}"))?;
        let (response, quit) = match serde_json::from_str::<HostRequest>(&line) {
            Ok(req) => server.handle(req),
            Err(e) => (error(format!("要求を読めない: {e}")), false),
        };
        let text = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        writeln!(out, "{text}")
            .and_then(|()| out.flush())
            .map_err(|e| format!("応答を書けない: {e}"))?;
        if quit {
            break;
        }
    }
    server.session.take();
    Ok(())
}

fn error(message: String) -> HostResponse {
    HostResponse::Error { message }
}

impl Server {
    fn handle(&mut self, req: HostRequest) -> (HostResponse, bool) {
        let pong = HostResponse::Pong {
            protocol_version: PROTOCOL_VERSION,
        };
        let result = match req {
            HostRequest::Ping => Ok(pong),
            HostRequest::Shutdown => return (pong, true),
            HostRequest::Prepare { params } => {
                self.prepare(params).map(|()| HostResponse::Prepared)
            }
            HostRequest::RenderFrame { time, size, params } => {
                self.render(time, size, params).map(HostResponse::Frame)
            }
        };
        // 期限を過ぎた要求の応答はアプリが読み捨てるので、理由を標準エラーにも残す。
        if let Err(e) = &result {
            eprintln!("mm-host-ofx: {e}");
        }
        (result.unwrap_or_else(error), false)
    }

    fn prepare(&mut self, params: serde_json::Value) -> Result<(), String> {
        let params: PrepareParams = serde_json::from_value(params)
            .map_err(|e| format!("Prepare の params が読めない: {e}"))?;
        self.session.take();
        let bundle = ManuallyDrop::new(Bundle::open(&params.bundle)?);
        let plugin = pick(bundle.plugins()?, params.plugin.as_deref())?;
        plugin.load()?;
        let described = plugin.describe().inspect_err(|_| plugin.unload())?;
        let mut session = Session {
            instance: None,
            plugin,
            described,
            _bundle: bundle,
        };
        let (context, in_context) = session.described.context.as_ref().ok_or_else(|| {
            format!(
                "{} にこのホストで描ける文脈（Filter、Generator）が無い",
                session.plugin.id
            )
        })?;
        let depths = in_context.props.strings(FX_SUPPORTED_DEPTHS);
        let depth = Depth::choose(&depths)
            .ok_or_else(|| format!("対応する画素の深さが無い: {depths:?}"))?;
        session.instance = Some(Instance::create(
            &session.plugin,
            in_context,
            context,
            depth,
            params.frame_rate,
        )?);
        self.session = Some(session);
        Ok(())
    }

    fn render(
        &mut self,
        time: RationalTime,
        size: Size,
        params: serde_json::Value,
    ) -> Result<SharedFrame, String> {
        let params: RenderParams = serde_json::from_value(params)
            .map_err(|e| format!("RenderFrame の params が読めない: {e}"))?;
        let instance = self
            .session
            .as_ref()
            .and_then(|s| s.instance.as_ref())
            .ok_or("Prepare の前に RenderFrame")?;
        let image = instance.render(time, size, &params)?;
        Ok(SharedFrame::from_image(&image))
    }
}

/// `id` のプラグイン。`None` なら最初の画像の効果。
fn pick(plugins: Vec<Plugin>, id: Option<&str>) -> Result<Plugin, String> {
    let ids: Vec<String> = plugins.iter().map(|p| p.id.clone()).collect();
    plugins
        .into_iter()
        .filter(Plugin::is_image_effect)
        .find(|p| id.is_none_or(|id| p.id == id))
        .ok_or_else(|| format!("プラグイン {id:?} が無い（あるのは {ids:?}）"))
}
