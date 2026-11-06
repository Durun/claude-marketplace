//! `Decoder` を通して d3d12va の 1 フレームの時間と fps を、mm-trace のスパンで測る。
//!
//! 使い方: `cargo run --release --features d3d12va --example bench_d3d12va -- <素材> <mode> [--frames N] [--warmup N]`
//!
//! mode:
//! - `decode`: `next_frame` を回し、フレームを破棄する。変換の提出までを含む。最後に GPU の完了を待つ。
//! - `render`: レンダラーと同じ手順（`import` → `before_submit` → 空の提出 → `after_submit`）まで回す。
//! - `seek`: 40 か所へシークし、頼んでから 1 フレーム目が出るまで。

use std::path::PathBuf;
use std::time::Instant;

use mm_decode_api::{Decoder, DecoderFactory, GpuContextRef};
use mm_decode_d3d12va::{gpu_request, D3d12vaFactory};
use mm_gpu::{GpuContext, WgpuGpu};
use mm_node_api::GpuHelper;
use mm_trace::MetricsLayer;
use mm_types::RationalTime;
use tracing_subscriber::layer::SubscriberExt;

struct Args {
    path: PathBuf,
    mode: String,
    frames: usize,
    warmup: usize,
}

fn parse() -> Args {
    let mut it = std::env::args().skip(1);
    let path = PathBuf::from(it.next().expect("素材のパス"));
    let mode = it.next().expect("mode");
    let mut a = Args {
        path,
        mode,
        frames: 300,
        warmup: 30,
    };
    while let Some(k) = it.next() {
        let v: usize = it.next().and_then(|v| v.parse().ok()).expect("数");
        match k.as_str() {
            "--frames" => a.frames = v,
            "--warmup" => a.warmup = v,
            _ => panic!("知らない引数 {k}"),
        }
    }
    a
}

fn next_or_rewind(dec: &mut dyn Decoder) -> mm_decode_api::DecodedFrame {
    if let Some(f) = dec.next_frame().expect("decode") {
        return f;
    }
    dec.seek(RationalTime::ZERO).expect("seek");
    dec.next_frame().expect("decode").expect("frame")
}

fn run(a: &Args, ctx: &std::sync::Arc<GpuContext>) -> (f64, usize) {
    let mut dec = D3d12vaFactory
        .open(&a.path, Some(ctx.as_ref() as &dyn GpuContextRef))
        .expect("open");
    let mut helper = WgpuGpu::new(ctx.clone());
    let mut started = Instant::now();
    let mut measured = 0usize;
    for i in 0..a.warmup + a.frames {
        if i == a.warmup {
            let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
            started = Instant::now();
        }
        let _frame_span = (i >= a.warmup).then(|| tracing::info_span!("decode.frame").entered());
        let f = next_or_rewind(dec.as_mut());
        if a.mode == "render" {
            let handle = f.gpu.expect("gpu");
            helper.import(&handle).expect("import");
            helper.before_submit();
            ctx.queue.submit([]);
            helper.after_submit();
        }
        if i >= a.warmup {
            measured += 1;
        }
    }
    let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
    (started.elapsed().as_secs_f64(), measured)
}

fn seek_bench(a: &Args, ctx: &std::sync::Arc<GpuContext>) {
    let mut dec = D3d12vaFactory
        .open(&a.path, Some(ctx.as_ref() as &dyn GpuContextRef))
        .expect("open");
    let fps = dec.info().fps.expect("cfr");
    let count = dec.info().frame_count().max(1);
    for k in 0..40i64 {
        let target = (k * 97 + 13) % (count - 5);
        let t = RationalTime::from_frames(target, fps).unwrap();
        let _s = tracing::info_span!("decode.seek").entered();
        dec.seek(t).expect("seek");
        let f = dec.next_frame().expect("next").expect("frame");
        if f.index != target {
            println!("シーク先 {target} に対して {} が出た", f.index);
        }
        let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

fn main() {
    let a = parse();
    let (layer, handle) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .expect("subscriber");
    let ctx = GpuContext::with(&gpu_request(None).expect("D3D12 のアダプター")).expect("gpu");
    println!("GPU: {}", ctx.adapter_name());
    if a.mode == "seek" {
        seek_bench(&a, &ctx);
    } else {
        let (secs, n) = run(&a, &ctx);
        println!(
            "{} {} {n} フレーム {secs:.3} s = {:.1} fps",
            a.path.file_name().unwrap().to_string_lossy(),
            a.mode,
            n as f64 / secs
        );
    }
    for (name, s) in handle.snapshot().spans {
        println!(
            "  {name:<18} n={:<5} p50 {:>7.3} ms  p95 {:>7.3} ms  max {:>7.3} ms",
            s.count,
            s.p50_us as f64 / 1000.0,
            s.p95_us as f64 / 1000.0,
            s.max_us as f64 / 1000.0
        );
    }
}
