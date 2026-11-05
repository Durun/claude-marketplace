//! 窓の組み立て。Tauri の窓の下に、左に wgpu の面（子 HWND）、右に子 WebView を並べる。

use std::sync::{Arc, Mutex};

use mm_trace::chrome::ChromeTraceHandle;
use mm_trace::{ChromeTraceLayer, MetricsHandle, MetricsLayer};
use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{
    Manager, PhysicalPosition, PhysicalSize, Position, Rect, Size, Webview, WebviewUrl, Window,
    WindowEvent,
};
use tracing::Span;
use tracing_subscriber::prelude::*;

use crate::gpu::Renderer;
use crate::shared::Shared;
use crate::sync::SyncState;
use crate::{selftest, surface, sync};

/// 面の幅の割合。残りが WebView。
const SPLIT: f64 = 0.6;

/// 配置を直すのに要るもの。
pub struct Ctx {
    pub shared: Arc<Shared>,
    pub metrics: MetricsHandle,
    pub chrome: ChromeTraceHandle,
    pub resize_span: Arc<Mutex<Option<Span>>>,
    pub surface_hwnd: isize,
}

/// 矩形（x、y、幅、高さ。物理ピクセル）。
pub type PixelRect = (i32, i32, i32, i32);

/// 面と WebView の矩形（物理ピクセル）を窓の大きさから決める。
pub fn split_rects(width: u32, height: u32) -> (PixelRect, PixelRect) {
    let left = (f64::from(width) * SPLIT).round() as i32;
    let (w, h) = (width as i32, height as i32);
    ((0, 0, left, h), (left, 0, w - left, h))
}

/// 窓の大きさに合わせて面と WebView を置き直す。
pub fn layout(window: &Window, webview: &Webview, ctx: &Ctx) {
    let Ok(size) = window.inner_size() else {
        return;
    };
    let (s, v) = split_rects(size.width, size.height);
    {
        let mut slot = ctx.resize_span.lock().expect("resize span");
        if slot.is_none() {
            *slot = Some(tracing::info_span!("window.resize"));
        }
    }
    surface::set_rect(ctx.surface_hwnd, s);
    let _ = webview.set_bounds(Rect {
        position: Position::Physical(PhysicalPosition::new(v.0, v.1)),
        size: Size::Physical(PhysicalSize::new(v.2.max(1) as u32, v.3.max(1) as u32)),
    });
    ctx.shared
        .set_surface_size(s.2.max(1) as u32, s.3.max(1) as u32);
    ctx.shared.layout_us.store(
        ctx.shared.now_us().max(1),
        std::sync::atomic::Ordering::SeqCst,
    );
}

pub fn run() {
    let (metrics_layer, metrics) = MetricsLayer::new();
    let (chrome_layer, chrome) = ChromeTraceLayer::new();
    tracing_subscriber::registry()
        .with(metrics_layer)
        .with(chrome_layer)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let selftest_out = args
        .iter()
        .position(|a| a == "--selftest")
        .and_then(|i| args.get(i + 1).cloned());

    let shared = Arc::new(Shared::new());
    let state = Arc::new(SyncState::new(shared.clone()));
    let resize_span = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            sync::subscribe,
            sync::sync_ack,
            sync::key_event,
            sync::ui_size
        ])
        .setup(move |app| {
            let window = WindowBuilder::new(app, "main")
                .title("movie-maker spike")
                .inner_size(1280.0, 800.0)
                .build()?;
            let size = window.inner_size()?;
            let (s, v) = split_rects(size.width, size.height);
            let parent = window.hwnd()?.0 as isize;
            let surface_hwnd = surface::create(parent, shared.clone(), s)?;
            let webview = window.add_child(
                WebviewBuilder::new("ui", WebviewUrl::App("index.html".into())),
                PhysicalPosition::new(v.0, v.1),
                PhysicalSize::new(v.2 as u32, v.3 as u32),
            )?;
            shared.set_surface_size(s.2 as u32, s.3 as u32);

            let ctx = Arc::new(Ctx {
                shared: shared.clone(),
                metrics,
                chrome,
                resize_span: resize_span.clone(),
                surface_hwnd,
            });
            let renderer = Renderer::spawn(shared.clone(), surface_hwnd, resize_span);
            sync::spawn(app.handle().clone(), state);

            let (w, wv, c) = (window.clone(), webview.clone(), ctx.clone());
            window.on_window_event(move |e| {
                if matches!(
                    e,
                    WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
                ) {
                    layout(&w, &wv, &c);
                }
            });
            // 描画スレッドは窓が閉じるまで止めない。取っ手は終了時に落とす。
            app.manage(Mutex::new(Some(renderer)));

            if let Some(out) = selftest_out.clone() {
                selftest::spawn(app.handle().clone(), window, ctx, out);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri の実行に失敗");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 面と_webview_は隙間も重なりもなく窓を埋める() {
        for (w, h) in [(1600, 1000), (1125, 750), (2400, 1350), (1, 1)] {
            let (s, v) = split_rects(w, h);
            assert_eq!(s.0 + s.2, v.0, "面の右端と WebView の左端が一致する");
            assert_eq!(v.0 + v.2, w as i32);
            assert_eq!((s.3, v.3), (h as i32, h as i32));
        }
    }
}
