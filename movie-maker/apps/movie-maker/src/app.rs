//! ウィンドウの組み立て。Tauri のウィンドウの下に、左に wgpu のサーフェス（子 HWND。Program と Timeline）、
//! 右に子 WebView（`ui/dist/` の shell と再生の操作列）を並べ、再生スレッドと描画スレッドをつなぐ。

use std::sync::{Arc, Mutex};

use mm_doc::Project;
use mm_trace::{ChromeTraceLayer, MetricsLayer};
use mm_types::{CompositionId, MmError};
use mm_ui_wgpu::timeline_model;
use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{
    PhysicalPosition, PhysicalSize, Position, Rect, Size, Webview, WebviewUrl, Window, WindowEvent,
};
use tracing_subscriber::prelude::*;
use windows::Win32::Media::timeBeginPeriod;

use crate::cli::{self, Args};
use crate::ipc::{self, LayerInfo, ProjectInfo, SyncState};
use crate::playback::{Playback, PlaybackInfo, PlayerOptions};
use crate::project;
use crate::render::Renderer;
use crate::state::Shared;
use crate::surface::{self, PixelRect};
use crate::verify::{self, Telemetry};

/// サーフェスの幅の割合。残りが WebView。
const SPLIT: f64 = 0.6;
/// ウィンドウの初期の大きさ（論理ピクセル）。
const INITIAL_SIZE: (f64, f64) = (1600.0, 900.0);
/// OS のタイマーの分解能（ms）。待ちの粒度が既定の 15.6 ms だと、フレームの間隔を守れない。
const TIMER_RESOLUTION_MS: u32 = 1;
/// WebView が最初に読むページ。`ui/dist/` の shell。
const UI_ENTRY: &str = "index.html";
/// WebView に重ねる再生の操作列。
const TRANSPORT_JS: &str = include_str!("../web/transport.js");

/// 描画スレッドと再生スレッド。ウィンドウが閉じる前に止めるため、`Option` で持って取り出して落とす。
struct Workers {
    renderer: Mutex<Option<Renderer>>,
    playback: Mutex<Option<Playback>>,
}

impl Workers {
    /// HWND が破棄される前に描画を止め、続けて再生を止める。何度呼んでもよい。
    fn stop(&self) {
        if let Some(mut r) = self.renderer.lock().expect("renderer").take() {
            r.stop();
        }
        drop(self.playback.lock().expect("playback").take());
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
    let c = opened.project.composition(comp)?;
    let layers = c
        .layers
        .iter()
        .rev()
        .filter_map(|l| {
            let r = l.range().ok()?;
            Some(LayerInfo {
                id: l.id.to_string(),
                kind: l.source.kind.clone(),
                start: r.start.to_f64(),
                end: r.end.to_f64(),
            })
        })
        .collect();
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
    let opened = project::open(&args.project).unwrap_or_else(|e| fail(&e.to_string()));
    let comp_id = project::pick_composition(&opened.project, args.composition.as_deref())
        .unwrap_or_else(|e| fail(&e.to_string()));

    let shared = Arc::new(Shared::new());
    let project = Arc::new(opened.project.clone());
    let options = PlayerOptions {
        no_audio: args.no_audio,
        config: None,
    };
    let (playback, pinfo) =
        Playback::spawn(shared.clone(), project.clone(), comp_id.clone(), options)
            .unwrap_or_else(|e| fail(&format!("再生を始められない: {e}")));
    let info = project_info(&opened, &comp_id, &pinfo).unwrap_or_else(|e| fail(&e.to_string()));
    let state = Arc::new(SyncState::new(shared.clone()));
    state.set_info(info);
    let workers = Arc::new(Workers {
        renderer: Mutex::new(None),
        playback: Mutex::new(Some(playback)),
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
    let Boot {
        shared,
        state,
        workers,
        project,
        comp_id,
        pinfo,
        telemetry,
        args,
    } = boot;
    let managed = state.clone();
    tauri::Builder::default()
        .manage(managed)
        .invoke_handler(tauri::generate_handler![
            ipc::subscribe,
            ipc::sync_ack,
            ipc::transport,
            ipc::project_info,
            ipc::key_event
        ])
        .setup(move |app| {
            let window = WindowBuilder::new(app, "main")
                .title("movie-maker")
                .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
                .build()?;
            let size = window.inner_size()?;
            let (s, v) = split_rects(size.width, size.height);
            let surface_hwnd = surface::create(window.hwnd()?.0 as isize, shared.clone(), s)?;
            let webview = window.add_child(
                WebviewBuilder::new("ui", WebviewUrl::App(UI_ENTRY.into()))
                    .initialization_script(TRANSPORT_JS),
                PhysicalPosition::new(v.0, v.1),
                PhysicalSize::new(v.2 as u32, v.3 as u32),
            )?;
            shared.set_surface_size(s.2 as u32, s.3 as u32);
            let model = timeline_model(project.composition(&comp_id)?);
            *workers.renderer.lock().expect("renderer") =
                Some(Renderer::spawn(shared.clone(), surface_hwnd, model));
            ipc::spawn(app.handle().clone(), state.clone());
            watch_window(
                &window,
                webview,
                shared.clone(),
                surface_hwnd,
                workers.clone(),
            );
            if let Some(test) = args.play_test.clone() {
                verify::spawn(
                    app.handle().clone(),
                    window,
                    shared.clone(),
                    pinfo.clone(),
                    telemetry,
                    test,
                );
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri の実行に失敗");
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
