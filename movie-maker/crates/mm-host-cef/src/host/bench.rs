//! 1 コマの時間を測る。時刻はこちらが決め、描いた絵を受け取るまでをスパンで測る。

use std::rc::Rc;
use std::time::{Duration, Instant};

use cef::*;
use mm_trace::MetricsLayer;
use tracing_subscriber::layer::SubscriberExt;

use super::handlers::{build_client, Shared};
use super::{gpu, BenchOptions};

const PAGE: &str = include_str!("../../assets/bench.html");
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const RESEND_LIMIT: u32 = 100_000;

/// 条件が成り立つまで CEF のメッセージを回す。待ちの間は寝ない（寝るとタイマーの粒度が測定に入る）。
fn pump_until(shared: &Shared, what: &str, cond: impl Fn(&Shared) -> bool) -> Result<(), String> {
    let start = Instant::now();
    while !cond(shared) {
        if let Some(e) = shared.error.borrow().as_ref() {
            return Err(e.clone());
        }
        if start.elapsed() > FRAME_TIMEOUT {
            return Err(format!("{what} が {FRAME_TIMEOUT:?} 来ない"));
        }
        do_message_loop_work();
        std::hint::spin_loop();
    }
    Ok(())
}

/// 時刻 `t` の画面にして、1 コマ受け取る。
fn render_frame(
    browser: &Browser,
    shared: &Shared,
    opts: &BenchOptions,
    seq: u64,
    t: f64,
) -> Result<(), String> {
    let _frame = tracing::info_span!("host_cef.frame").entered();
    {
        let _span = tracing::info_span!("host_cef.set_time").entered();
        let main = browser.main_frame().ok_or("メインフレームが無い")?;
        let code = format!("mmSetTime({t},{seq})");
        main.execute_java_script(Some(&CefString::from(code.as_str())), None, 0);
        let want = format!("ready:{seq}");
        pump_until(shared, "時刻の反映", |s| *s.title.borrow() == want)?;
    }
    let _span = tracing::info_span!("host_cef.begin_frame").entered();
    let before = shared.painted.get();
    let host = browser.host().ok_or("ブラウザのホストが無い")?;
    // 描画の準備ができる前に送ったフレームは捨てられるので、来なければ送り直す。
    for attempt in 0..RESEND_LIMIT {
        host.send_external_begin_frame();
        let start = Instant::now();
        while shared.painted.get() == before && start.elapsed() < opts.resend {
            if let Some(e) = shared.error.borrow().as_ref() {
                return Err(e.clone());
            }
            do_message_loop_work();
            std::hint::spin_loop();
        }
        if shared.painted.get() > before {
            if attempt > 0 {
                mm_trace::counter!("host_cef.begin_frame_resent", attempt);
            }
            return Ok(());
        }
    }
    Err(format!("描画が {RESEND_LIMIT} 回送っても来ない"))
}

fn create_browser(shared: &Rc<Shared>, opts: &BenchOptions, url: &str) -> Result<Browser, String> {
    let window_info = WindowInfo {
        windowless_rendering_enabled: true as _,
        shared_texture_enabled: shared.accelerated as _,
        external_begin_frame_enabled: true as _,
        ..Default::default()
    };
    let browser_settings = BrowserSettings {
        windowless_frame_rate: opts.rate as _,
        ..Default::default()
    };
    browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut build_client(shared)),
        Some(&CefString::from(url)),
        Some(&browser_settings),
        None,
        None,
    )
    .ok_or_else(|| "ブラウザを作れない".to_string())
}

fn read_hash(shared: &Shared) -> Result<(u64, Vec<u8>), String> {
    let frame = shared.frame.borrow();
    let texture = frame.as_ref().ok_or("絵が無い")?;
    let rgba = gpu::read_rgba(&shared.device, &shared.queue, texture)?;
    Ok((gpu::fnv1a(&rgba), rgba))
}

/// ページが左上に塗る 16px の四角から、どの `seq` の絵かを読む。古い絵が来ていないかの確認に使う。
fn marker_seq(rgba: &[u8], width: u32) -> u64 {
    let at = ((8 * width + 8) * 4) as usize;
    u64::from(rgba[at]) | u64::from(rgba[at + 1]) << 8 | u64::from(rgba[at + 2]) << 16
}

pub fn run(opts: &BenchOptions) -> Result<(), String> {
    let (layer, metrics) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .map_err(|e| e.to_string())?;

    let (device, queue, adapter) = gpu::create_device(&opts.adapter)?;
    let path = if opts.accelerated {
        "accel (OnAcceleratedPaint, 共有テクスチャ)"
    } else {
        "cpu (OnPaint, BGRA のアップロード)"
    };
    println!(
        "adapter: {adapter}\npath: {path}\nsize: {}x{}  frames: {} (warmup {})",
        opts.size.0, opts.size.1, opts.frames, opts.warmup
    );

    let dir = std::env::temp_dir().join("mm-host-cef-page");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("bench.html"), PAGE).map_err(|e| e.to_string())?;
    let url = format!(
        "file:///{}",
        dir.join("bench.html").to_string_lossy().replace('\\', "/")
    );

    let shared = Shared::new(device, queue, opts.accelerated, opts.size);
    let browser = create_browser(&shared, opts, &url)?;
    pump_until(&shared, "読み込み", |s| {
        s.loaded.get() && !s.title.borrow().is_empty()
    })?;

    let dt = 1.0 / f64::from(opts.fps);
    let mut seq = 0u64;
    for _ in 0..opts.warmup {
        render_frame(&browser, &shared, opts, seq, seq as f64 * dt)?;
        seq += 1;
    }
    metrics.drain();

    let (mut hashes, mut shots, mut stale) = (Vec::new(), Vec::new(), 0u32);
    let wall = Instant::now();
    for i in 0..opts.frames {
        render_frame(&browser, &shared, opts, seq, seq as f64 * dt)?;
        if opts.verify {
            let (hash, rgba) = read_hash(&shared)?;
            if marker_seq(&rgba, opts.size.0) != seq {
                stale += 1;
            }
            hashes.push((seq, hash));
            if i == 0 || i == opts.frames / 2 || i + 1 == opts.frames {
                shots.push((seq, rgba));
            }
        }
        seq += 1;
    }
    let wall = wall.elapsed();

    if opts.verify {
        println!("古い時刻の絵が来たコマ: {stale} / {}", opts.frames);
        verify(&browser, &shared, dt, &hashes, &shots, opts)?;
    }
    report(&metrics.snapshot(), wall, opts);
    if let Some(host) = browser.host() {
        host.close_browser(1);
    }
    let end = Instant::now();
    while end.elapsed() < Duration::from_millis(300) {
        do_message_loop_work();
    }
    Ok(())
}

/// 同じ時刻を描き直して同じ絵になるか、PNG に書く。
fn verify(
    browser: &Browser,
    shared: &Shared,
    dt: f64,
    hashes: &[(u64, u64)],
    shots: &[(u64, Vec<u8>)],
    opts: &BenchOptions,
) -> Result<(), String> {
    for (s, h) in hashes {
        println!("hash seq={s} {h:016x}");
    }
    let (first_seq, first_hash) = hashes
        .get(hashes.len() / 2)
        .copied()
        .ok_or("検証するコマが無い")?;
    render_frame(browser, shared, opts, first_seq, first_seq as f64 * dt)?;
    let (again, _) = read_hash(shared)?;
    println!(
        "再描画 seq={first_seq}: {first_hash:016x} -> {again:016x} {}",
        if again == first_hash {
            "同一"
        } else {
            "不一致"
        }
    );
    if let Some(out) = &opts.out {
        std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
        for (s, rgba) in shots {
            let p = out.join(format!(
                "frame-{}-{s:04}.png",
                if opts.accelerated { "accel" } else { "cpu" }
            ));
            gpu::write_png(&p, opts.size.0, opts.size.1, rgba)?;
            println!("png: {}", p.display());
        }
    }
    Ok(())
}

fn report(snap: &mm_trace::MetricsSnapshot, wall: Duration, opts: &BenchOptions) {
    println!("\nspan                 count    p50 ms   p95 ms   p99 ms   max ms");
    for (name, s) in &snap.spans {
        let ms = |us: u64| us as f64 / 1000.0;
        println!(
            "{name:<20} {:>5} {:>9.2} {:>8.2} {:>8.2} {:>8.2}",
            s.count,
            ms(s.p50_us),
            ms(s.p95_us),
            ms(s.p99_us),
            ms(s.max_us)
        );
    }
    println!(
        "wall: {:.1} ms / {} frames = {:.2} ms/frame ({:.1} fps)",
        wall.as_secs_f64() * 1000.0,
        opts.frames,
        wall.as_secs_f64() * 1000.0 / f64::from(opts.frames),
        f64::from(opts.frames) / wall.as_secs_f64()
    );
    if let Some(out) = &opts.out {
        let line = snap.to_jsonl_line();
        let name = if opts.accelerated {
            "metrics-accel.jsonl"
        } else {
            "metrics-cpu.jsonl"
        };
        if let Err(e) =
            std::fs::create_dir_all(out).and_then(|()| std::fs::write(out.join(name), line + "\n"))
        {
            eprintln!("metrics の書き出しに失敗: {e}");
        }
    }
}
