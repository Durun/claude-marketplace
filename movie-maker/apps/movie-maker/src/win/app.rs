//! ウィンドウの組み立て。Tauri のウィンドウの下に、左に wgpu のサーフェス（子 HWND。Program と Timeline）、
//! 右に子 WebView（`ui/dist/` の shell と再生の操作列）を並べ、再生スレッドと描画スレッドをつなぐ。

use std::sync::{Arc, Mutex};

use mm_doc::Project;
use mm_serve::{Core, EngineFrames, FrameSource, Server};
use mm_trace::{ChromeTraceLayer, MetricsLayer};
use mm_types::{CompositionId, MmError};
use mm_ui_wgpu::timeline_model;
use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{DragDropEvent, Emitter, Manager, WebviewEvent};
use tauri::{
    PhysicalPosition, PhysicalSize, Position, Rect, Size, Webview, WebviewUrl, Window, WindowEvent,
};
use tracing_subscriber::prelude::*;
use windows::Win32::Media::timeBeginPeriod;

use crate::win::cli::{self, Args};
use crate::win::deliver::DeliverState;
use crate::win::deliver_ipc;
use crate::win::edit;
use crate::win::ipc::{self, ProjectInfo, SyncState};
use crate::win::media::{self, MediaState};
use crate::win::media_ipc;
use crate::win::mixer_ipc;
use crate::win::pack_ipc;
use crate::win::playback::{Playback, PlaybackInfo, PlayerOptions};
use crate::win::project;
use crate::win::render::Renderer;
use crate::win::review_ipc::{self, ReviewState};
use crate::win::source_ipc;
use crate::win::state::Shared;
use crate::win::story::StoryState;
use crate::win::story_ipc;
use crate::win::surface::{self, PixelRect};
use crate::win::transition_ipc;
use crate::win::verify::{self, Telemetry};

/// サーフェスの幅の割合。残りが WebView。
const SPLIT: f64 = 0.5;
/// ウィンドウの初期の大きさ（論理ピクセル）。
const INITIAL_SIZE: (f64, f64) = (1600.0, 900.0);
/// OS のタイマーの分解能（ms）。待ちの粒度が既定の 15.6 ms だと、フレームの間隔を守れない。
const TIMER_RESOLUTION_MS: u32 = 1;
/// WebView が最初に読むページ。`ui/dist/` の shell。
const UI_ENTRY: &str = "index.html";
/// WebView に重ねる再生の操作列。
const TRANSPORT_JS: &str = include_str!("../../web/transport.js");
/// 保存した変更を WebView に知らせる Tauri のイベント名。中身は `mm_serve::Change`。
const DOC_CHANGED_EVENT: &str = "mm:doc-changed";
/// 提案の追加と決定を Review パネルへ知らせる Tauri のイベント。
const PROPOSALS_CHANGED_EVENT: &str = "mm:proposals-changed";

/// 描画スレッドと再生スレッド。ウィンドウが閉じる前に止めるため、`Option` で持って取り出して落とす。
struct Workers {
    renderer: Mutex<Option<Renderer>>,
    playback: Mutex<Option<Playback>>,
    /// HTTP サーバー。落とすと `.mm/` のポートとトークンが消える。
    server: Mutex<Option<Server>>,
    /// 書き手の `project.mmdb`。再生が止まった後に閉じて、バックアップと WAL の書き戻しをする。
    core: Option<Arc<Core>>,
}

impl Workers {
    /// HWND が破棄される前に描画を止め、続けて再生を止める。何度呼んでもよい。
    fn stop(&self) {
        if let Some(mut r) = self.renderer.lock().expect("renderer").take() {
            r.stop();
        }
        drop(self.playback.lock().expect("playback").take());
        drop(self.server.lock().expect("server").take());
        if let Some(core) = &self.core {
            if let Err(e) = core.close() {
                tracing::warn!("プロジェクトを閉じられない: {e}");
            }
        }
    }
}

/// 配置を直すのに要るもの。
struct Layout {
    shared: Arc<Shared>,
    surface_hwnd: isize,
}

/// サーフェスと WebView の矩形（物理ピクセル）をウィンドウの大きさから決める。
pub fn split_rects(width: u32, height: u32) -> (PixelRect, PixelRect) {
    let left = (f64::from(width) * SPLIT).round() as i32;
    let (w, h) = (width as i32, height as i32);
    ((0, 0, left, h), (left, 0, w - left, h))
}

fn layout(window: &Window, webview: &Webview, l: &Layout) {
    let Ok(size) = window.inner_size() else {
        return;
    };
    let (s, v) = split_rects(size.width, size.height);
    surface::set_rect(l.surface_hwnd, s);
    let _ = webview.set_bounds(Rect {
        position: Position::Physical(PhysicalPosition::new(v.0, v.1)),
        size: Size::Physical(PhysicalSize::new(v.2.max(1) as u32, v.3.max(1) as u32)),
    });
    l.shared
        .set_surface_size(s.2.max(1) as u32, s.3.max(1) as u32);
}

fn init_tracing() -> Telemetry {
    let (metrics_layer, metrics) = MetricsLayer::new();
    let (chrome_layer, chrome) = ChromeTraceLayer::new();
    tracing_subscriber::registry()
        .with(metrics_layer)
        .with(chrome_layer)
        .init();
    Telemetry { metrics, chrome }
}

fn project_info(
    opened: &project::Opened,
    comp: &CompositionId,
    playback: &PlaybackInfo,
) -> Result<ProjectInfo, MmError> {
    let layers = ipc::layer_infos(opened.project.composition(comp)?);
    Ok(ProjectInfo {
        name: opened.name.clone(),
        composition: comp.to_string(),
        width: playback.width,
        height: playback.height,
        fps: playback.fps,
        duration: playback.duration,
        clock: playback.clock.to_owned(),
        audio_device: playback.audio_device.clone(),
        backends: playback.backends.clone(),
        layers,
        adapter: String::new(),
    })
}

/// 書き手として開いた `project.mmdb` を `Core` に渡し、HTTP サーバーを起動する。サーバーを起動できなくても、
/// IPC の操作は `Core` を通して保存する。
fn start_serve(shared: &Shared, store: Option<mm_store::Store>) -> Option<Server> {
    let store = store?;
    let config = mm_runtime::RuntimeConfig {
        present: false,
        ..crate::win::playback::runtime_config()
    };
    let frames = match EngineFrames::spawn(config) {
        Ok(f) => Some(Arc::new(f) as Arc<dyn FrameSource>),
        Err(e) => {
            tracing::warn!("フレームの描画を始められない（GET /frame は使えない）: {e}");
            None
        }
    };
    let core = Core::new(store, frames);
    let _ = shared.core.set(core.clone());
    match Server::start(core) {
        Ok(server) => {
            eprintln!("HTTP サーバー: 127.0.0.1:{}", server.port());
            Some(server)
        }
        Err(e) => {
            tracing::warn!("HTTP サーバーを起動できない: {e}");
            None
        }
    }
}

/// 保存した変更を再生スレッドと WebView に伝える。HTTP と IPC のどちらから書かれても同じ道を通る。
/// WebView への通知は `Shared::doc` を置き直した後に送るので、受けたパネルが読み直すと新しいデータが返る。
fn forward_changes(app: &tauri::AppHandle, shared: &Arc<Shared>) {
    let handle = app.clone();
    let started = edit::watch_changes(shared, move |change| {
        let _ = handle.emit(DOC_CHANGED_EVENT, change);
    });
    if let Err(e) = started {
        tracing::warn!("変更の通知を受けるスレッドを始められない: {e}");
    }
    let Some(core) = shared.core.get() else {
        return;
    };
    let handle = app.clone();
    let started = core.on_proposal(move |event| {
        let _ = handle.emit(PROPOSALS_CHANGED_EVENT, event);
    });
    if let Err(e) = started {
        tracing::warn!("提案の通知を受けるスレッドを始められない: {e}");
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

pub fn run() {
    unsafe {
        timeBeginPeriod(TIMER_RESOLUTION_MS);
    }
    let telemetry = init_tracing();
    let args =
        cli::parse(&std::env::args().skip(1).collect::<Vec<_>>()).unwrap_or_else(|e| fail(&e));
    let mut opened = project::open(&args.project).unwrap_or_else(|e| fail(&e.to_string()));
    let comp_id = project::pick_composition(&opened.project, args.composition.as_deref())
        .unwrap_or_else(|e| fail(&e.to_string()));

    let shared = Arc::new(Shared::new());
    let server = start_serve(&shared, opened.store.take());
    if args.play_test.is_some() {
        shared.collect_series();
    }
    let project = Arc::new(opened.project.clone());
    let options = PlayerOptions {
        no_audio: args.no_audio,
        config: None,
        store_dir: mm_store::store_dir(&args.project),
    };
    shared.source.configure(options.clone());
    let (playback, pinfo) =
        Playback::spawn(shared.clone(), project.clone(), comp_id.clone(), options)
            .unwrap_or_else(|e| fail(&format!("再生を始められない: {e}")));
    let info = project_info(&opened, &comp_id, &pinfo).unwrap_or_else(|e| fail(&e.to_string()));
    let state = Arc::new(SyncState::new(shared.clone()));
    state.set_info(info);
    let workers = Arc::new(Workers {
        renderer: Mutex::new(None),
        playback: Mutex::new(Some(playback)),
        server: Mutex::new(server),
        core: shared.core.get().cloned(),
    });
    let boot = Boot {
        shared,
        state,
        workers,
        project,
        comp_id,
        pinfo,
        telemetry,
        args,
    };
    build_and_run(boot);
}

/// ウィンドウを作る前に揃えたもの。
struct Boot {
    shared: Arc<Shared>,
    state: Arc<SyncState>,
    workers: Arc<Workers>,
    project: Arc<Project>,
    comp_id: CompositionId,
    pinfo: PlaybackInfo,
    telemetry: Telemetry,
    args: Args,
}

fn build_and_run(boot: Boot) {
    let managed = boot.state.clone();
    let media_state = Arc::new(MediaState::new(
        boot.shared.clone(),
        media::project_cache_dir(&boot.args.project),
    ));
    let story_state = Arc::new(StoryState::new(
        boot.shared.clone(),
        &boot.args.project,
        media::project_cache_dir(&boot.args.project),
    ));
    let deliver_state = Arc::new(DeliverState::new(boot.shared.clone(), &boot.args.project));
    let review_state = Arc::new(ReviewState::new(boot.shared.clone()));
    tauri::Builder::default()
        .manage(managed)
        .manage(deliver_state)
        .manage(media_state)
        .manage(story_state)
        .manage(review_state)
        .invoke_handler(dispatch)
        .setup(move |app| boot.setup(app))
        .run(tauri::generate_context!())
        .expect("tauri の実行に失敗");
}

/// コマンドを受け持つ処理。
#[derive(Debug, PartialEq, Eq)]
enum Panel {
    Common,
    Deliver,
    Media,
    Mixer,
    Pack,
    Review,
    Source,
    Story,
    Transition,
}

/// コマンド名の前置きで受け持つ処理を決める。前置きが合わないものは、所属を明示した名前だけが例外。
fn route(command: &str) -> Panel {
    match command {
        "edit_action" => Panel::Source,
        c if c.starts_with("export_") => Panel::Deliver,
        c if c.starts_with("media_") => Panel::Media,
        c if c.starts_with("mixer_") => Panel::Mixer,
        c if c.starts_with("pack_") => Panel::Pack,
        c if c.starts_with("review_") => Panel::Review,
        c if c.starts_with("source_") => Panel::Source,
        c if c.starts_with("story_") => Panel::Story,
        c if c.starts_with("transition_") => Panel::Transition,
        _ => Panel::Common,
    }
}

/// 各処理の `generate_handler!` は知らない名前に `false` を返すので、振り分けた先で未知の名前が弾かれる。
fn dispatch(invoke: tauri::ipc::Invoke) -> bool {
    let handler: fn(tauri::ipc::Invoke) -> bool = match route(invoke.message.command()) {
        Panel::Common => ipc::handler,
        Panel::Deliver => deliver_ipc::handler,
        Panel::Media => media_ipc::handler,
        Panel::Mixer => mixer_ipc::handler,
        Panel::Pack => pack_ipc::handler,
        Panel::Review => review_ipc::handler,
        Panel::Source => source_ipc::handler,
        Panel::Story => story_ipc::handler,
        Panel::Transition => transition_ipc::handler,
    };
    handler(invoke)
}

type SetupResult = Result<(), Box<dyn std::error::Error>>;

impl Boot {
    /// ウィンドウと描画スレッドを作り、同期と自動試験を始める。
    fn setup(self, app: &mut tauri::App) -> SetupResult {
        let (window, webview, surface_hwnd) = self.create_window(app)?;
        let model = timeline_model(&self.project, self.project.composition(&self.comp_id)?);
        *self.workers.renderer.lock().expect("renderer") = Some(Renderer::spawn(
            self.shared.clone(),
            surface_hwnd,
            model.clone(),
        ));
        ipc::spawn(app.handle().clone(), self.state.clone());
        forward_changes(app.handle(), &self.shared);
        offer_drops(&webview, app.state::<Arc<MediaState>>().inner().clone());
        watch_window(
            &window,
            webview,
            self.shared.clone(),
            surface_hwnd,
            self.workers.clone(),
        );
        if let Some(test) = self.args.play_test {
            let handle = app.handle().clone();
            let target = verify::Target {
                window,
                surface_hwnd,
                model: model.clone(),
            };
            verify::spawn(
                handle,
                target,
                self.shared,
                self.pinfo,
                self.telemetry,
                test,
            );
        }
        Ok(())
    }

    /// ウィンドウの下に、サーフェスの子 HWND と子 WebView を並べる。サーフェスの HWND を返す。
    fn create_window(
        &self,
        app: &mut tauri::App,
    ) -> Result<(Window, Webview, isize), Box<dyn std::error::Error>> {
        let window = WindowBuilder::new(app, "main")
            .title("movie-maker")
            .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
            .build()?;
        let size = window.inner_size()?;
        let (s, v) = split_rects(size.width, size.height);
        let surface_hwnd = surface::create(window.hwnd()?.0 as isize, self.shared.clone(), s)?;
        let webview = window.add_child(
            WebviewBuilder::new("ui", WebviewUrl::App(UI_ENTRY.into()))
                .initialization_script(TRANSPORT_JS),
            PhysicalPosition::new(v.0, v.1),
            PhysicalSize::new(v.2 as u32, v.3 as u32),
        )?;
        self.shared.set_surface_size(s.2 as u32, s.3 as u32);
        Ok((window, webview, surface_hwnd))
    }
}

/// WebView にドロップされたファイルのパスを、Media の取り込んでよいパスとして覚える。
fn offer_drops(webview: &Webview, media: Arc<MediaState>) {
    webview.on_webview_event(move |e| {
        if let WebviewEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = e {
            media.offer(paths.iter().cloned());
        }
    });
}

/// 大きさの変更に配置を合わせ、閉じるときに HWND より先に描画と再生を止める。
fn watch_window(
    window: &Window,
    webview: Webview,
    shared: Arc<Shared>,
    surface_hwnd: isize,
    workers: Arc<Workers>,
) {
    let l = Layout {
        shared: shared.clone(),
        surface_hwnd,
    };
    let w = window.clone();
    window.on_window_event(move |e| match e {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            layout(&w, &webview, &l)
        }
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
            shared.quit.store(true, std::sync::atomic::Ordering::SeqCst);
            workers.stop();
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn コマンド名は前置きで受け持つ処理に振り分けられる() {
        let cases = [
            ("subscribe", Panel::Common),
            ("apply_ops", Panel::Common),
            ("export_start", Panel::Deliver),
            ("media_list", Panel::Media),
            ("mixer_state", Panel::Mixer),
            ("pack_apply", Panel::Pack),
            ("review_undo", Panel::Review),
            ("source_open", Panel::Source),
            ("edit_action", Panel::Source),
            ("story_search", Panel::Story),
            ("transition_cuts", Panel::Transition),
            ("unknown_command", Panel::Common),
        ];
        for (name, panel) in cases {
            assert_eq!(route(name), panel, "{name}");
        }
    }

    #[test]
    fn サーフェスと_webview_は隙間も重なりもなくウィンドウを埋める() {
        for (w, h) in [(1600, 1000), (1125, 750), (2400, 1350), (1, 1)] {
            let (s, v) = split_rects(w, h);
            assert_eq!(
                s.0 + s.2,
                v.0,
                "サーフェスの右端と WebView の左端が一致する"
            );
            assert_eq!(v.0 + v.2, w as i32);
            assert_eq!((s.3, v.3), (h as i32, h as i32));
        }
    }
}
