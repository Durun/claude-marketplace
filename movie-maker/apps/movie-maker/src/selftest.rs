//! `--selftest <out.json>` の自己診断。段階ごとに窓を動かして数字を取り、JSON に書いて終わる。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, LogicalSize, Monitor, PhysicalPosition, Window};

use crate::app::{split_rects, Ctx};
use crate::input;
use crate::shared::{SYNC_CHANNEL, SYNC_EMIT, SYNC_OFF};

const VK_J: u16 = 0x4A;
const VK_K: u16 = 0x4B;
const VK_L: u16 = 0x4C;
/// キーを送る回数（J、K、L を 7 回ずつ）。
const KEY_ROUNDS: usize = 7;

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// 自己診断のスレッドを始める。終わったらアプリを閉じる。
pub fn spawn(app: AppHandle, window: Window, ctx: Arc<Ctx>, out: String) {
    std::thread::Builder::new()
        .name("selftest".into())
        .spawn(move || {
            let result = run(&window, &ctx, &out);
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            if let Err(e) = std::fs::write(&out, text) {
                eprintln!("結果を書けない: {e}");
            }
            let mut trace = Vec::new();
            if ctx.chrome.write_to(&mut trace).is_ok() {
                let _ = std::fs::write(format!("{out}.trace.json"), trace);
            }
            app.exit(0);
        })
        .expect("selftest thread");
}

fn run(window: &Window, ctx: &Ctx, out: &str) -> Value {
    sleep(3000);
    let top = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    input::bring_to_front(top);
    sleep(500);
    let foreground = input::is_foreground_ours();

    let mut r = json!({ "foreground_ok": foreground });
    r["adapter"] = json!(ctx.shared.adapter.lock().expect("adapter").clone());
    r["stage_idle_no_sync"] = stage(ctx, SYNC_OFF, 5.0);
    r["stage_sync_channel"] = stage(ctx, SYNC_CHANNEL, 10.0);
    r["stage_sync_emit"] = stage(ctx, SYNC_EMIT, 10.0);
    r["stage_resize_steps"] = resize_steps(window, ctx);
    r["stage_resize_drag"] = resize_drag(window, ctx);
    if foreground {
        r["stage_keys"] = keys(window, ctx, top);
    } else {
        r["stage_keys"] = json!("前面に出せなかったので、入力を送らなかった");
    }
    r["stage_dpi"] = dpi(window, ctx);
    r["stage_soak_60s"] = stage(ctx, SYNC_CHANNEL, 60.0);
    ctx.shared.sync_mode.store(SYNC_CHANNEL, Ordering::SeqCst);
    let _ = std::fs::write(format!("{out}.ready"), "ready");
    sleep(6000);
    let _ = window.set_always_on_top(false);
    r
}

/// 同期の方式を決めて `secs` 秒走らせ、描画の fps と各系列を集める。
fn stage(ctx: &Ctx, mode: u8, secs: f64) -> Value {
    ctx.shared.sync_mode.store(mode, Ordering::SeqCst);
    sleep(500);
    let mark = Mark::now(ctx);
    sleep((secs * 1000.0) as u64);
    collect(ctx, mode, &mark)
}

/// 測り始めの印。系列と span の集計を空にして、フレーム数と時刻を控える。
struct Mark {
    at: Instant,
    frames: u64,
    errors: u64,
}

impl Mark {
    fn now(ctx: &Ctx) -> Self {
        let _ = ctx.metrics.drain();
        let _ = ctx.shared.drain_series();
        Self {
            at: Instant::now(),
            frames: ctx.shared.frames.load(Ordering::SeqCst),
            errors: ctx.shared.surface_errors.load(Ordering::SeqCst),
        }
    }
}

fn collect(ctx: &Ctx, mode: u8, mark: &Mark) -> Value {
    let secs = mark.at.elapsed().as_secs_f64();
    let frames = ctx.shared.frames.load(Ordering::SeqCst) - mark.frames;
    let errors = ctx.shared.surface_errors.load(Ordering::SeqCst) - mark.errors;
    let spans: serde_json::Map<String, Value> = ctx
        .metrics
        .drain()
        .spans
        .into_iter()
        .map(|(k, s)| {
            let v = json!({"count": s.count, "p50_us": s.p50_us, "p95_us": s.p95_us,
                           "p99_us": s.p99_us, "max_us": s.max_us});
            (k, v)
        })
        .collect();
    json!({
        "sync_mode": mode,
        "secs": secs,
        "frames": frames,
        "fps": frames as f64 / secs,
        "surface_errors": errors,
        "series_ms": ctx.shared.drain_series(),
        "mm_trace_spans": spans,
    })
}

/// 窓の大きさを段階的に変え、面と WebView の大きさが合うかと、変更から present までを測る。
fn resize_steps(window: &Window, ctx: &Ctx) -> Value {
    ctx.shared.sync_mode.store(SYNC_CHANNEL, Ordering::SeqCst);
    let mark = Mark::now(ctx);
    let mut checks = Vec::new();
    for (w, h) in [
        (1600.0, 1000.0),
        (900.0, 600.0),
        (1920.0, 1080.0),
        (1280.0, 800.0),
    ] {
        let _ = window.set_size(LogicalSize::new(w, h));
        sleep(900);
        checks.push(layout_check(window, ctx, (w, h)));
    }
    let mut v = collect(ctx, SYNC_CHANNEL, &mark);
    v["checks"] = json!(checks);
    v
}

/// 面の矩形（HWND の実測）と WebView の報告した大きさが、窓の大きさから決めた値に合うか。
fn layout_check(window: &Window, ctx: &Ctx, logical: (f64, f64)) -> Value {
    let inner = window.inner_size().unwrap_or_default();
    let scale = window.scale_factor().unwrap_or(1.0);
    let (s, v) = split_rects(inner.width, inner.height);
    let ui = *ctx.shared.ui_size.lock().expect("ui_size");
    let surface = ctx.shared.surface_size();
    let hwnd = ctx.surface_hwnd;
    let r = input::window_rect(hwnd);
    json!({
        "logical_requested": [logical.0, logical.1],
        "inner_physical": [inner.width, inner.height],
        "scale": scale,
        "surface_expected": [s.2, s.3],
        "surface_state": [surface.0, surface.1],
        "surface_hwnd_rect": [r.2 - r.0, r.3 - r.1],
        "webview_expected_css": [f64::from(v.2) / scale, f64::from(v.3) / scale],
        "webview_reported_css": [ui.width, ui.height],
        "webview_dpr": ui.dpr,
        "ok": surface == (s.2 as u32, s.3 as u32)
            && (ui.width - f64::from(v.2) / scale).abs() <= 1.5
            && (ui.height - f64::from(v.3) / scale).abs() <= 1.5,
    })
}

/// ドラッグ中のように 10 ms ごとに大きさを変え続ける。
fn resize_drag(window: &Window, ctx: &Ctx) -> Value {
    let mark = Mark::now(ctx);
    for i in 0..=100 {
        let _ = window.set_size(LogicalSize::new(1000.0 + 6.0 * f64::from(i), 700.0));
        sleep(10);
    }
    for i in 0..=100 {
        let _ = window.set_size(LogicalSize::new(
            1600.0 - 3.2 * f64::from(i),
            700.0 + 1.0 * f64::from(i),
        ));
        sleep(10);
    }
    sleep(500);
    let _ = window.set_size(LogicalSize::new(1280.0, 800.0));
    sleep(700);
    let mut v = collect(ctx, SYNC_CHANNEL, &mark);
    v["final_check"] = layout_check(window, ctx, (1280.0, 800.0));
    v
}

/// 面と WebView を順にクリックして J/K/L を送り、どちらに届くか、いつ Rust に届くかを見る。
fn keys(window: &Window, ctx: &Ctx, top: isize) -> Value {
    let inner = window.inner_size().unwrap_or_default();
    let (s, v) = split_rects(inner.width, inner.height);
    let surface_pt = input::client_to_screen(top, s.2 / 2, s.3 / 2);
    let web_pt = input::client_to_screen(top, v.0 + v.2 / 2, v.3 * 3 / 4);
    let mut result = json!({});
    for (name, pt) in [("surface_clicked", surface_pt), ("webview_clicked", web_pt)] {
        input::click_screen(pt.0, pt.1);
        sleep(500);
        let (hwnd, class) = input::focused(top);
        let _ = ctx.shared.drain_series();
        let (s0, w0) = (
            ctx.shared.keys_surface.load(Ordering::SeqCst),
            ctx.shared.keys_webview.load(Ordering::SeqCst),
        );
        for _ in 0..KEY_ROUNDS {
            for vk in [VK_J, VK_K, VK_L] {
                ctx.shared
                    .input_stamp_us
                    .store(ctx.shared.now_us(), Ordering::SeqCst);
                input::tap_key(vk);
                sleep(60);
            }
        }
        sleep(300);
        result[name] = json!({
            "focus_hwnd_is_surface": hwnd == ctx.surface_hwnd,
            "focus_class": class,
            "sent": KEY_ROUNDS * 3,
            "arrived_surface": ctx.shared.keys_surface.load(Ordering::SeqCst) - s0,
            "arrived_webview": ctx.shared.keys_webview.load(Ordering::SeqCst) - w0,
            "series_ms": ctx.shared.drain_series(),
        });
    }
    result
}

/// DPI の倍率と、別の倍率のモニターへ動かしたときの配置。
fn dpi(window: &Window, ctx: &Ctx) -> Value {
    let monitors: Vec<Monitor> = window.available_monitors().unwrap_or_default();
    let list: Vec<Value> = monitors
        .iter()
        .map(|m| {
            json!({"position": [m.position().x, m.position().y],
                   "size": [m.size().width, m.size().height],
                   "scale": m.scale_factor()})
        })
        .collect();
    let home = window.outer_position().unwrap_or_default();
    let here = window.scale_factor().unwrap_or(1.0);
    let mut moves = Vec::new();
    // 倍率が同じモニターでも、別のアダプターにつながっていると面が落ちうるので全部へ動かす。
    for m in &monitors {
        let (mx, my) = (m.position().x, m.position().y);
        if (mx..mx + m.size().width as i32).contains(&home.x)
            && (my..my + m.size().height as i32).contains(&home.y)
        {
            continue;
        }
        let mark = Mark::now(ctx);
        let _ = window.set_position(PhysicalPosition::new(mx + 200, my + 200));
        sleep(3000);
        let mut moved = collect(ctx, SYNC_CHANNEL, &mark);
        moved["to_scale"] = json!(m.scale_factor());
        moved["check"] = layout_check(window, ctx, (0.0, 0.0));
        moves.push(moved);
        let mark = Mark::now(ctx);
        let _ = window.set_position(home);
        sleep(2000);
        let mut back = collect(ctx, SYNC_CHANNEL, &mark);
        back["back_scale"] = json!(here);
        back["check"] = layout_check(window, ctx, (0.0, 0.0));
        moves.push(back);
    }
    json!({"home_scale": here, "monitors": list, "moves": moves})
}
