//! `mm-host-cef serve`: 標準入力の `HostRequest` に 1 行ずつ `HostResponse` で答える。
//!
//! ページの約束（`Prepare` の `html`）: `window.mmSetTime(t, params)` を定義し、時刻 `t`（秒）と
//! `RenderFrame` の `params` の関数としてだけ画面を作る。定義が無いページは静止画として描く。
//! ページはホストが一時フォルダーに書いたファイルとして開き、ほかのファイルとネットワークは読めない（`guard`）。
//! ページはビューポート（`RenderFrame` の `size`）に収める。ぼかしなどのフィルタの付いた要素がビューポートの端を
//! またぐと、1 フレームが 20 ms ほど遅れる（`docs/design/spikes/cef.md`）。

use std::io::{BufRead, Write};
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use cef::*;
use mm_gpu::GpuContext;
use mm_host_api::{HostRequest, HostResponse, HostSpec, SharedFrame, PROTOCOL_VERSION};
use mm_types::{RationalTime, Size};

use super::args::{DEFAULT_RATE, DEFAULT_RESEND};
use super::handlers::Shared;
use super::share::Client;
use super::{browser, gpu, guard};

/// 終わるときにスパンの分布（`metrics.jsonl` の 1 行）を書くファイル。
const METRICS_ENV: &str = "MM_HOST_METRICS";
/// 要求が無い間に CEF のメッセージを回す間隔。
const IDLE_PUMP: Duration = Duration::from_millis(1);
/// 最初のビューポートの大きさ（最初の `RenderFrame` で頼まれた大きさに変える）。
const INITIAL_SIZE: (u32, u32) = (1280, 720);
/// BeginFrame からフレームが来るまで待つ上限。過ぎたら前のフレームを返さずに誤りにする。
/// アプリの 1 フレームの期限（`mm-nodes-cef` と `mm-text-cef` の 5 秒）より短くし、理由を応答で返せるようにする。
const FRAME_LIMIT: Duration = Duration::from_secs(4);
/// ページの読み込み後に足す、時刻の反映と反映済みの合図（タイトル）を行う関数。
///
/// - 書体: 時刻の反映で初めて使う Web フォントは、組んだ時点で読み込みが始まる。組み直してから
///   `document.fonts` の読み込みが終わるまで待ち、読み込み中の代わりの書体で描かないようにする。
/// - 毎フレームのダメージ: 画面が変わらないフレームは CEF がフレームを出さず、遅いページと区別できない。
///   左上の透明な 1 画素の canvas を毎回消し直してダメージを作り、どのフレームも必ず届くようにする
///   （透明の画素は合成しても画素を変えない）。
const RUNTIME_JS: &str = "(() => {\
  const c = document.createElement('canvas'); c.width = 1; c.height = 1;\
  c.setAttribute('style', 'position:fixed!important;left:0!important;top:0!important;\
width:8px!important;height:8px!important;margin:0!important;display:block!important;\
visibility:visible!important;opacity:1!important;z-index:2147483647!important;pointer-events:none!important');\
  document.documentElement.appendChild(c);\
  const g = c.getContext('2d');\
  const settle = (done) => { void document.documentElement.getBoundingClientRect();\
    if (document.fonts.status === 'loaded') { done(); } else { document.fonts.ready.then(() => settle(done)); } };\
  window.__mmFrame = (t, seq, params) => {\
    if (typeof window.mmSetTime === 'function') { window.mmSetTime(t, params); }\
    settle(() => { g.clearRect(0, 0, 1, 1); document.title = 'mm-ready:' + seq; }); };\
  document.title = 'mm-init'; })();";

struct Server {
    shared: Rc<Shared>,
    browser: Option<Browser>,
    client: Option<Client>,
    page_dir: std::path::PathBuf,
    seq: u64,
}

/// 標準入力が閉じるか `Shutdown` が来るまで答え続ける。
pub fn run(spec: &HostSpec, gpu: Arc<GpuContext>) -> Result<(), String> {
    let metrics = install_metrics()?;
    let client = spec.client_pid.map(Client::open).transpose()?;
    let shared = Shared::new(gpu, INITIAL_SIZE);
    shared.share.set(client.is_some());
    let mut server = Server {
        shared,
        browser: None,
        client,
        page_dir: super::work_dir().0.join("pages"),
        seq: 0,
    };
    let (tx, rx) = mpsc::channel();
    // 標準入力は別スレッドで読む。CEF のメッセージは本体のスレッドで回し続ける必要がある。
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut out = std::io::stdout().lock();
    loop {
        let line = match rx.recv_timeout(IDLE_PUMP) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => {
                do_message_loop_work();
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
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
    finish(server, metrics)
}

/// ブラウザーを閉じ、ページのフォルダーを消し、集めたスパンの分布を書く。
fn finish(
    mut server: Server,
    metrics: Option<(String, mm_trace::MetricsHandle)>,
) -> Result<(), String> {
    if let Some(browser) = server.browser.take() {
        browser::close(&browser);
    }
    // 片付けに失敗しても次の起動は別の pid のフォルダーを使う。
    let _ = std::fs::remove_dir_all(&server.page_dir);
    if let Some((path, handle)) = metrics {
        let line = handle.snapshot().to_jsonl_line() + "\n";
        std::fs::write(&path, line).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

/// 環境変数 `MM_HOST_METRICS` にファイルがあれば、スパンの分布を集めて終わるときにそこへ書く。
fn install_metrics() -> Result<Option<(String, mm_trace::MetricsHandle)>, String> {
    use tracing_subscriber::layer::SubscriberExt;
    let Ok(path) = std::env::var(METRICS_ENV) else {
        return Ok(None);
    };
    let (layer, handle) = mm_trace::MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .map_err(|e| e.to_string())?;
    Ok(Some((path, handle)))
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
                self.prepare(&params).map(|()| HostResponse::Prepared)
            }
            HostRequest::RenderFrame { time, size, params } => {
                self.render(time, size, &params).map(HostResponse::Frame)
            }
        };
        // 期限を過ぎた要求の応答はアプリが読み捨てるので、理由を標準エラーにも残す。
        if let Err(e) = &result {
            eprintln!("mm-host-cef: {e}");
        }
        (result.unwrap_or_else(error), false)
    }

    fn page_url(&self, params: &serde_json::Value) -> Result<String, String> {
        let html = params
            .get("html")
            .and_then(serde_json::Value::as_str)
            .ok_or("Prepare の params に html が要る")?;
        std::fs::create_dir_all(&self.page_dir).map_err(|e| e.to_string())?;
        let path = self.page_dir.join(format!("page-{}.html", self.seq));
        std::fs::write(&path, html).map_err(|e| e.to_string())?;
        Ok(format!(
            "file:///{}",
            path.to_string_lossy().replace('\\', "/")
        ))
    }

    /// ページを読み込み、時刻を反映する関数を足す。前のページは閉じる。
    fn prepare(&mut self, params: &serde_json::Value) -> Result<(), String> {
        let url = self.page_url(params)?;
        if let Some(old) = self.browser.take() {
            browser::close(&old);
        }
        *self.shared.frame.borrow_mut() = None;
        self.shared.loaded.set(false);
        self.shared.title.borrow_mut().clear();
        guard::reset(&self.shared.page);
        let created = browser::create(&self.shared, true, DEFAULT_RATE, &url)?;
        browser::pump_until(&self.shared, "読み込み", browser::WAIT_LIMIT, |s| {
            s.loaded.get()
        })?;
        browser::run_js(&created, &self.shared, RUNTIME_JS, "mm-init")?;
        self.browser = Some(created);
        Ok(())
    }

    /// ビューポートを `size` にする。CEF は `view_rect` を問い直して描き直す。
    fn resize(&self, browser: &Browser, size: Size) -> Result<(), String> {
        let wanted = (size.width, size.height);
        if self.shared.size.get() != wanted {
            self.shared.size.set(wanted);
            browser
                .host()
                .ok_or("ブラウザのホストが無い")?
                .was_resized();
        }
        Ok(())
    }

    fn render(
        &mut self,
        time: RationalTime,
        size: Size,
        params: &serde_json::Value,
    ) -> Result<SharedFrame, String> {
        let _span = tracing::info_span!("host_cef.frame").entered();
        let browser = self.browser.clone().ok_or("Prepare の前に RenderFrame")?;
        self.resize(&browser, size)?;
        self.seq += 1;
        let seq = self.seq;
        let js = format!("window.__mmFrame({}, {seq}, {params})", time.to_f64());
        browser::run_js(&browser, &self.shared, &js, &format!("mm-ready:{seq}"))?;
        let wanted = (size.width, size.height);
        let accept = |s: &Shared| s.frame_size() == Some(wanted);
        let got =
            browser::begin_frame(&browser, &self.shared, DEFAULT_RESEND, FRAME_LIMIT, accept)?;
        if !got {
            return Err(format!(
                "{}x{} のフレームが {FRAME_LIMIT:?} 以内に来ない（ページの描画が遅すぎる）",
                size.width, size.height
            ));
        }
        self.output(size)
    }

    /// 最後のフレームを、共有テクスチャのハンドルか CPU の画像にして返す。
    fn output(&self, size: Size) -> Result<SharedFrame, String> {
        let frame = self.shared.frame.borrow();
        let frame = frame.as_ref().ok_or("フレームが無い")?;
        if let (Some(client), Some(resource)) = (&self.client, &frame.shared) {
            let _span = tracing::info_span!("host_cef.share").entered();
            let handle = client.export(&self.shared.gpu.device, resource)?;
            return Ok(SharedFrame::SharedTexture {
                handle,
                size,
                fence: None,
            });
        }
        let bgra = gpu::read_bgra(&self.shared.gpu, &frame.texture)?;
        Ok(SharedFrame::Cpu {
            size,
            data: mm_host_cef::pixel::bgra8_to_linear_rgba(&bgra),
        })
    }
}
