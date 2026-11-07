//! `--play-test` の自動の再生試験。プロジェクトを先頭から最後まで再生して、ドロップしたフレームの数、
//! 映像と音のずれ、WebView の時計の誤差を測り、途中のスクリーンショットと一緒に残して終わる。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_trace::chrome::ChromeTraceHandle;
use mm_trace::MetricsHandle;
use mm_types::ParamValue;
use mm_ui_layout::{row_top, split_surface, TimeView, TimelineModel, ROW_H, RULER_H, VIEW_MARGIN};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Window};

use crate::win::capture;
use crate::win::cli::PlayTest;
use crate::win::playback::{Command, PlaybackInfo};
use crate::win::state::Shared;
use crate::win::surface;
use crate::win::verify_edit;

/// 描画が安定して WebView の初期化が終わるまでの待ち。
const SETTLE: Duration = Duration::from_millis(2500);
/// 再生が始まるのを待つ上限。
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// 再生の時間を超えて待つ余裕。
const FINISH_SLACK: Duration = Duration::from_secs(5);
/// 状態を見に行く間隔。
const POLL: Duration = Duration::from_millis(25);

/// スクラブで狙う時刻（秒）。
const SCRUB_TARGET: f64 = 3.0;
/// 入力を投げてから結果を見るまでの待ち。
const INPUT_WAIT: Duration = Duration::from_millis(400);
/// スペースキーの仮想キーコード。
const VK_SPACE: u32 = 0x20;
/// Inspector で入れる不透明度。
const INSPECTOR_OPACITY: f64 = 0.5;
/// 選択や値の変更が反映されるのを待つ上限。
const FLOW_TIMEOUT: Duration = Duration::from_secs(3);
/// WebView の Inspector の不透明度の入力欄に値を入れて change を投げる。
const SET_OPACITY_JS: &str = r#"(() => {
  const input = document.querySelector('.mm-shell-right [aria-label="opacity"]');
  if (!input) return;
  input.value = "0.5";
  input.dispatchEvent(new Event("change", { bubbles: true }));
})()"#;

/// 試験の対象。ウィンドウと、サーフェスの子 HWND と、Timeline のモデル。
pub struct Target {
    pub window: Window,
    pub surface_hwnd: isize,
    pub model: TimelineModel,
}

/// 計測の出力先。
pub struct Telemetry {
    pub metrics: MetricsHandle,
    pub chrome: ChromeTraceHandle,
}

/// 再生の前の数字。再生の区間だけを数えるために引く。
struct Baseline {
    dropped: u64,
    rendered: u64,
    presented: u64,
    underruns: u64,
}

impl Baseline {
    fn take(shared: &Shared) -> Self {
        Self {
            dropped: shared.dropped_frames.load(Ordering::SeqCst),
            rendered: shared.rendered_frames.load(Ordering::SeqCst),
            presented: shared.frames_presented.load(Ordering::SeqCst),
            underruns: shared.clock.get().map_or(0, |c| c.underruns()),
        }
    }
}

/// 試験のスレッドを始める。終わったらアプリを閉じる。
pub fn spawn(
    app: AppHandle,
    target: Target,
    shared: Arc<Shared>,
    info: PlaybackInfo,
    telemetry: Telemetry,
    test: PlayTest,
) {
    std::thread::Builder::new()
        .name("play-test".into())
        .spawn(move || {
            let report = run(&app, &target, &shared, &info, &telemetry, &test);
            let text = serde_json::to_string_pretty(&report).unwrap_or_default();
            if let Err(e) = std::fs::write(&test.report, text) {
                eprintln!("結果を書けない: {e}");
            }
            write_telemetry(&telemetry, &test);
            app.exit(0);
        })
        .expect("play-test thread");
}

fn write_telemetry(t: &Telemetry, test: &PlayTest) {
    let line = t.metrics.snapshot().to_jsonl_line();
    let _ = std::fs::write(test.report.with_extension("metrics.jsonl"), line + "\n");
    let mut trace = Vec::new();
    if t.chrome.write_to(&mut trace).is_ok() {
        let _ = std::fs::write(test.report.with_extension("trace.json"), trace);
    }
}

pub(crate) fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(POLL);
    }
    false
}

fn run(
    app: &AppHandle,
    target: &Target,
    shared: &Arc<Shared>,
    info: &PlaybackInfo,
    telemetry: &Telemetry,
    test: &PlayTest,
) -> Value {
    let window = &target.window;
    let hwnd = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    wait_until(Duration::from_secs(20), || {
        shared.frames_presented.load(Ordering::SeqCst) > 30
            && !shared.adapter.lock().expect("adapter").is_empty()
    });
    std::thread::sleep(SETTLE);
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    shared.send(Command::Seek(0.0));
    std::thread::sleep(POLL * 4);
    let _ = shared.drain_series();
    let base = Baseline::take(shared);
    let _ = telemetry.metrics.drain();

    shared.send(Command::Play);
    let started = wait_until(START_TIMEOUT, || shared.playing.load(Ordering::SeqCst));
    let t0 = Instant::now();
    let (shot, shot_at) = play_and_capture(shared, info, test, hwnd, t0);
    let wall = t0.elapsed().as_secs_f64();
    let mut report = report(shared, info, test, &base, started, wall, shot);
    report["screenshot_at"] = shot_at;
    report["interaction"] = interaction(shared, info, target);
    report["inspector"] = inspector_flow(app, shared, target, hwnd, test);
    report["timeline_edit"] = verify_edit::run(&verify_edit::Env {
        shared,
        surface_hwnd: target.surface_hwnd,
        window_hwnd: hwnd,
        test,
    });
    let _ = window.set_always_on_top(false);
    report
}

/// 再生が終わるまで待ち、半分まで進んだところでスクリーンショットを撮る。撮った時の再生位置と、Program に
/// 渡した最新のフレームの時刻も返す。WebView の時刻の表示は画面への反映がサーフェスより約 2 フレーム遅れるので、
/// スクリーンショットの画像と比べるのはこちらの時刻にする。
fn play_and_capture(
    shared: &Shared,
    info: &PlaybackInfo,
    test: &PlayTest,
    hwnd: isize,
    t0: Instant,
) -> (Option<String>, Value) {
    let mut shot = None;
    let mut at = Value::Null;
    let half = Duration::from_secs_f64((info.duration.min(test.secs) / 2.0).max(0.1));
    wait_until(
        Duration::from_secs_f64(info.duration) + FINISH_SLACK,
        || {
            if shot.is_none() && t0.elapsed() >= half {
                at = json!({
                    "playhead_secs": shared.playhead(),
                    "frame_secs": shared.frames.newer_than(0).map(|f| f.time),
                });
                shot = test.screenshot.as_ref().map(|path| {
                    match capture::capture_window(hwnd, path) {
                        Ok(()) => path.display().to_string(),
                        Err(e) => format!("失敗: {e}"),
                    }
                });
                if shot.is_none() {
                    shot = Some(String::new());
                }
            }
            !shared.playing.load(Ordering::SeqCst)
        },
    );
    (shot.filter(|s| !s.is_empty()), at)
}

fn report(
    shared: &Shared,
    info: &PlaybackInfo,
    test: &PlayTest,
    base: &Baseline,
    started: bool,
    wall: f64,
    shot: Option<String>,
) -> Value {
    let now = Baseline::take(shared);
    let series = shared.drain_series();
    let dropped = now.dropped - base.dropped;
    let rendered = now.rendered - base.rendered;
    let presented = now.presented - base.presented;
    let total = (info.duration * info.fps).round();
    json!({
        "play_started": started,
        "final_position_secs": shared.playhead(),
        "playing_at_end": shared.playing.load(Ordering::SeqCst),
        "requested_secs": test.secs,
        "content_secs": info.duration,
        "wall_secs": wall,
        "clock": info.clock,
        "audio_device": info.audio_device,
        "adapter": shared.adapter.lock().expect("adapter").clone(),
        "backends": info.backends,
        "frames_total": total,
        "frames_rendered": rendered,
        "frames_dropped": dropped,
        "drop_rate_percent": if total > 0.0 { dropped as f64 / total * 100.0 } else { 0.0 },
        "frames_presented": presented,
        "present_fps": presented as f64 / wall.max(1e-9),
        "audio_underruns": now.underruns - base.underruns,
        "series": series,
        "screenshot": shot,
    })
}

/// 実際のウィンドウへの入力で、Timeline のスクラブとキー操作が効くか確かめる。
fn interaction(shared: &Shared, info: &PlaybackInfo, target: &Target) -> Value {
    let (w, h) = shared.surface_size();
    let tl = split_surface(w as f32, h as f32, target.model.content_height()).timeline;
    let view = TimeView::fit(target.model.duration, tl.w, VIEW_MARGIN);
    let x = tl.x + view.x_of(SCRUB_TARGET);
    surface::post_click(target.surface_hwnd, x as i32, (tl.y + RULER_H / 2.0) as i32);
    std::thread::sleep(INPUT_WAIT);
    let scrubbed = shared.playhead();
    let playing = || shared.playing.load(Ordering::SeqCst);
    let before = playing();
    surface::post_key(target.surface_hwnd, VK_SPACE);
    std::thread::sleep(INPUT_WAIT);
    let started = playing();
    surface::post_key(target.surface_hwnd, VK_SPACE);
    std::thread::sleep(INPUT_WAIT);
    let stopped = !playing();
    json!({
        "scrub_target_secs": SCRUB_TARGET,
        "scrub_result_secs": scrubbed,
        "scrub_ok": (scrubbed - SCRUB_TARGET).abs() <= 2.0 / info.fps,
        "space_starts_playback": !before && started,
        "space_stops_playback": stopped,
    })
}

fn layer_opacity(shared: &Shared, id: &str) -> Option<f64> {
    let doc = shared.doc()?;
    let comp = doc.project.composition(&doc.comp).ok()?;
    let layer = comp.layers.iter().find(|l| l.id.as_str() == id)?;
    match layer.opacity.value {
        ParamValue::Number(n) => Some(n),
        _ => None,
    }
}

fn shoot(hwnd: isize, test: &PlayTest, suffix: &str) -> Option<String> {
    let path = test
        .screenshot
        .as_ref()?
        .with_extension(format!("{suffix}.png"));
    capture::capture_window(hwnd, &path)
        .ok()
        .map(|()| path.display().to_string())
}

/// 実際のウィンドウで、Timeline のレイヤーを選ぶ → Inspector に出る → 値を変える → `apply_op` → 描き直し、の道を通す。
/// 選択と値の反映は共有状態で確かめ、Inspector の表示はスクリーンショットで目視する。
fn inspector_flow(
    app: &AppHandle,
    shared: &Shared,
    target: &Target,
    hwnd: isize,
    test: &PlayTest,
) -> Value {
    let (w, h) = shared.surface_size();
    let tl = split_surface(w as f32, h as f32, target.model.content_height()).timeline;
    let view = TimeView::fit(target.model.duration, tl.w, VIEW_MARGIN);
    let Some(item) = target.model.items.first() else {
        return json!({ "skipped": "レイヤーが無い" });
    };
    let x = tl.x + view.x_of((item.start + item.end) / 2.0);
    surface::post_click(
        target.surface_hwnd,
        x as i32,
        (tl.y + row_top(0) + ROW_H / 2.0) as i32,
    );
    let selected = wait_until(FLOW_TIMEOUT, || shared.selected_layer().is_some());
    let id = shared.selected_layer();
    std::thread::sleep(INPUT_WAIT);
    let shot_selected = shoot(hwnd, test, "inspector-selected");
    let rendered_before = shared.rendered_frames.load(Ordering::SeqCst);
    if let Some(webview) = app.get_webview("ui") {
        let _ = webview.eval(SET_OPACITY_JS);
    }
    let applied = id.as_deref().is_some_and(|id| {
        wait_until(FLOW_TIMEOUT, || {
            layer_opacity(shared, id) == Some(INSPECTOR_OPACITY)
        })
    });
    std::thread::sleep(INPUT_WAIT);
    let rerendered = shared.rendered_frames.load(Ordering::SeqCst) > rendered_before;
    json!({
        "selected": selected,
        "selected_layer": id,
        "opacity_applied": applied,
        "rerendered_after_apply": rerendered,
        "screenshot_selected": shot_selected,
        "screenshot_applied": shoot(hwnd, test, "inspector-applied"),
    })
}
