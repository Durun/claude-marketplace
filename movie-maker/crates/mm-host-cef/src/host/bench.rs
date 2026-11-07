//! 1 フレームの時間を測る。時刻はこちらが決め、描いたフレームを受け取るまでをスパンで測る。

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use cef::*;
use mm_gpu::GpuContext;
use mm_trace::MetricsLayer;
use tracing_subscriber::layer::SubscriberExt;

use super::handlers::Shared;
use super::share::Client;
use super::{browser, gpu, BenchOptions};

const PAGE: &str = include_str!("../../assets/bench.html");

struct Bench<'a> {
    browser: Browser,
    shared: Rc<Shared>,
    opts: &'a BenchOptions,
    /// `--share` のとき、自分のプロセスへハンドルを複製して受け渡しまで測る。
    client: Option<Client>,
}

impl Bench<'_> {
    /// 時刻 `t` の画面にして、1 フレーム受け取る。
    fn frame(&self, seq: u64, t: f64) -> Result<(), String> {
        let _frame = tracing::info_span!("host_cef.frame").entered();
        let (browser, shared) = (&self.browser, &*self.shared);
        let js = format!("mmSetTime({t},{seq})");
        browser::run_js(browser, shared, &js, &format!("ready:{seq}"))?;
        let got = browser::begin_frame(
            browser,
            shared,
            self.opts.resend,
            browser::WAIT_LIMIT,
            |_| true,
        )?;
        if !got {
            return Err(format!("seq {seq} のフレームが来ない"));
        }
        if let Some(client) = &self.client {
            self.export(client)?;
        }
        Ok(())
    }

    fn export(&self, client: &Client) -> Result<(), String> {
        let _span = tracing::info_span!("host_cef.share").entered();
        let frame = self.shared.frame.borrow();
        let resource = frame
            .as_ref()
            .and_then(|f| f.shared.as_ref())
            .ok_or("共有テクスチャが無い")?;
        let handle = client.export(&self.shared.gpu.device, resource)?;
        // 自分のプロセスへ複製したハンドルなので、受け取った側として閉じる。
        let raw = windows::Win32::Foundation::HANDLE(handle as usize as *mut _);
        // SAFETY: export が返した、このプロセスで有効なハンドル。
        unsafe { windows::Win32::Foundation::CloseHandle(raw) }
            .map_err(|e| format!("ハンドルを閉じられない: {e}"))
    }

    fn read_hash(&self) -> Result<(u64, Vec<u8>), String> {
        let frame = self.shared.frame.borrow();
        let texture = &frame.as_ref().ok_or("フレームが無い")?.texture;
        let rgba = gpu::bgra_to_rgba(&gpu::read_bgra(&self.shared.gpu, texture)?);
        Ok((gpu::fnv1a(&rgba), rgba))
    }
}

/// ページが左上に塗る 16px の四角から、どの `seq` のフレームかを読む。古いフレームが来ていないかの確認に使う。
fn marker_seq(rgba: &[u8], width: u32) -> u64 {
    let at = ((8 * width + 8) * 4) as usize;
    u64::from(rgba[at]) | u64::from(rgba[at + 1]) << 8 | u64::from(rgba[at + 2]) << 16
}

fn page_url(opts: &BenchOptions) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!("mm-host-cef-page-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // `--page-css` は原因の切り分け用。ページの先頭に CSS を足す。
    let page = PAGE.replace("<style>", &format!("<style>{}", opts.page_css));
    std::fs::write(dir.join("bench.html"), page).map_err(|e| e.to_string())?;
    Ok(format!(
        "file:///{}",
        dir.join("bench.html").to_string_lossy().replace('\\', "/")
    ))
}

/// 結果を表で出す。`--out` があれば `metrics-*.jsonl` も書く。
pub fn run(opts: &BenchOptions, gpu: Arc<GpuContext>) -> Result<(), String> {
    let (layer, metrics) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .map_err(|e| e.to_string())?;
    let path = match (opts.accelerated, opts.share) {
        (true, true) => "accel + share（共有ヒープへコピーし、ハンドルを複製）",
        (true, false) => "accel (OnAcceleratedPaint, 共有テクスチャ)",
        (false, _) => "cpu (OnPaint, BGRA のアップロード)",
    };
    println!(
        "adapter: {}\npath: {path}\nsize: {}x{}  frames: {} (warmup {})",
        gpu.adapter.name, opts.size.0, opts.size.1, opts.frames, opts.warmup
    );
    let shared = Shared::new(gpu, opts.size);
    shared.share.set(opts.share);
    let browser = browser::create(&shared, opts.accelerated, opts.rate, &page_url(opts)?)?;
    browser::pump_until(&shared, "読み込み", browser::WAIT_LIMIT, |s| {
        s.loaded.get() && !s.title.borrow().is_empty()
    })?;
    let client = match opts.share {
        true => Some(Client::open(std::process::id())?),
        false => None,
    };
    let bench = Bench {
        browser,
        shared,
        opts,
        client,
    };
    let result = measure(&bench, &metrics);
    browser::close(&bench.browser);
    result
}

fn measure(bench: &Bench, metrics: &mm_trace::MetricsHandle) -> Result<(), String> {
    let opts = bench.opts;
    let dt = 1.0 / f64::from(opts.fps);
    let mut seq = 0u64;
    for _ in 0..opts.warmup {
        bench.frame(seq, seq as f64 * dt)?;
        seq += 1;
    }
    metrics.drain();
    let (mut hashes, mut shots, mut stale) = (Vec::new(), Vec::new(), 0u32);
    let wall = Instant::now();
    for i in 0..opts.frames {
        bench.frame(seq, seq as f64 * dt)?;
        if opts.verify {
            let (hash, rgba) = bench.read_hash()?;
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
        println!(
            "古い時刻のフレームが来たフレーム: {stale} / {}",
            opts.frames
        );
        verify(bench, dt, &hashes, &shots)?;
    }
    report(&metrics.snapshot(), wall, opts);
    Ok(())
}

/// 同じ時刻を描き直して同じ画像になるか、PNG に書く。
fn verify(
    bench: &Bench,
    dt: f64,
    hashes: &[(u64, u64)],
    shots: &[(u64, Vec<u8>)],
) -> Result<(), String> {
    let (first_seq, first_hash) = hashes
        .get(hashes.len() / 2)
        .copied()
        .ok_or("検証するフレームが無い")?;
    bench.frame(first_seq, first_seq as f64 * dt)?;
    let (again, _) = bench.read_hash()?;
    let same = if again == first_hash {
        "同一"
    } else {
        "不一致"
    };
    println!("再描画 seq={first_seq}: {first_hash:016x} -> {again:016x} {same}");
    let opts = bench.opts;
    if let Some(out) = &opts.out {
        std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
        let kind = if opts.accelerated { "accel" } else { "cpu" };
        for (s, rgba) in shots {
            let p = out.join(format!("frame-{kind}-{s:04}.png"));
            gpu::write_png(&p, opts.size.0, opts.size.1, rgba)?;
            println!("png: {}", p.display());
        }
    }
    Ok(())
}

fn report(snap: &mm_trace::MetricsSnapshot, wall: std::time::Duration, opts: &BenchOptions) {
    println!("\nspan                 count    p50 ms   p95 ms   p99 ms   max ms");
    let ms = |us: u64| us as f64 / 1000.0;
    for (name, s) in &snap.spans {
        println!(
            "{name:<20} {:>5} {:>9.2} {:>8.2} {:>8.2} {:>8.2}",
            s.count,
            ms(s.p50_us),
            ms(s.p95_us),
            ms(s.p99_us),
            ms(s.max_us)
        );
    }
    for (name, v) in &snap.counters {
        println!("{name}: {v}");
    }
    let total_ms = wall.as_secs_f64() * 1000.0;
    let frames = f64::from(opts.frames);
    println!(
        "wall: {total_ms:.1} ms / {} frames = {:.2} ms/frame ({:.1} fps)",
        opts.frames,
        total_ms / frames,
        frames * 1000.0 / total_ms
    );
    let Some(out) = &opts.out else {
        return;
    };
    let name = match (opts.accelerated, opts.share) {
        (true, true) => "metrics-share.jsonl",
        (true, false) => "metrics-accel.jsonl",
        (false, _) => "metrics-cpu.jsonl",
    };
    let line = snap.to_jsonl_line() + "\n";
    if let Err(e) = std::fs::create_dir_all(out).and_then(|()| std::fs::write(out.join(name), line))
    {
        eprintln!("metrics の書き出しに失敗: {e}");
    }
}
