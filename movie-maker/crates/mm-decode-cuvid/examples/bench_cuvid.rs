//! cuvid の復号から wgpu までの 1 コマの時間を、mm-trace のスパンで測る。
//!
//! 使い方: `cargo run --release --features cuvid --example bench_cuvid -- <素材> <mode> [--frames N] [--inflight K]`
//!
//! mode:
//! - `decode`: 復号だけ（CUDA のメモリに置く。wgpu に渡さない）。
//! - `copy`: 復号 → 共有バッファへ 1 回コピー → NV12 を RGBA に変換して完了まで待つ。
//! - `hash`: コマ 10 を RGBA にして FNV-1a のハッシュを出す（d3d12va の経路と比べる）。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use mm_decode_cuvid::convert::BufferConverter;
use mm_decode_cuvid::cuda::CudaApi;
use mm_decode_cuvid::dec::CuvidDecoder;
use mm_decode_cuvid::gpu::GpuContext;
use mm_decode_cuvid::share::SharedRing;
use mm_trace::MetricsLayer;
use mm_types::Size;
use tracing_subscriber::layer::SubscriberExt;

/// 輪の枠の数。`--inflight` より大きくする。
const RING: usize = 8;

struct Args {
    path: PathBuf,
    mode: String,
    frames: usize,
    warmup: usize,
    inflight: usize,
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
        inflight: 0,
    };
    while let Some(k) = it.next() {
        let v: usize = it.next().and_then(|v| v.parse().ok()).expect("数");
        match k.as_str() {
            "--frames" => a.frames = v,
            "--warmup" => a.warmup = v,
            "--inflight" => a.inflight = v,
            _ => panic!("知らない引数 {k}"),
        }
    }
    a
}

fn wait(gpu: &GpuContext, sub: wgpu::SubmissionIndex) {
    let _s = tracing::info_span!("decode.gpu_wait").entered();
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(sub),
            timeout: None,
        })
        .expect("poll");
}

fn setup(a: &Args, gpu: &Arc<GpuContext>) -> (CuvidDecoder, SharedRing, BufferConverter) {
    let dec = CuvidDecoder::open(&a.path).expect("open");
    let size = Size::new(dec.decoder.width(), dec.decoder.height());
    let cuda = Arc::new(CudaApi::load().expect("nvcuda"));
    // SAFETY: 文脈とストリームは、いま開いた復号器のもの。
    let ring = unsafe {
        SharedRing::new(
            gpu.clone(),
            cuda,
            dec.cuda_ctx,
            dec.cuda_stream,
            (size.width, size.height),
            RING,
        )
    }
    .expect("ring");
    let conv = BufferConverter::new(gpu, size, &ring);
    (dec, ring, conv)
}

fn next_frame(dec: &mut CuvidDecoder) -> ffmpeg_next::frame::Video {
    let _s = tracing::info_span!("decode.receive").entered();
    match dec.next_cuda_frame().expect("decode") {
        Some(f) => f,
        None => {
            dec.rewind().expect("rewind");
            dec.next_cuda_frame().expect("decode").expect("frame")
        }
    }
}

fn run(a: &Args, gpu: &Arc<GpuContext>) -> (f64, usize) {
    let (mut dec, mut ring, conv) = setup(a, gpu);
    let mut pending: VecDeque<wgpu::SubmissionIndex> = VecDeque::new();
    let total = a.warmup + a.frames;
    let mut started = Instant::now();
    let mut measured = 0;
    for i in 0..total {
        if i == a.warmup {
            while let Some(s) = pending.pop_front() {
                wait(gpu, s);
            }
            started = Instant::now();
        }
        let frame_span = (i >= a.warmup)
            .then(|| tracing::info_span!("decode.frame", mode = a.mode.as_str()).entered());
        let f = next_frame(&mut dec);
        match a.mode.as_str() {
            "decode" => drop(f),
            "copy" => {
                let slot = {
                    let _s = tracing::info_span!("decode.copy").entered();
                    ring.copy_frame(&f).expect("copy")
                };
                drop(f);
                ring.wait_copy_on(slot).expect("wait");
                let sub = {
                    let _s = tracing::info_span!("decode.convert").entered();
                    conv.submit(gpu, slot)
                };
                pending.push_back(sub);
            }
            other => panic!("知らない mode {other}"),
        }
        while pending.len() > a.inflight {
            let s = pending.pop_front().expect("pending");
            wait(gpu, s);
        }
        drop(frame_span);
        if i >= a.warmup {
            measured += 1;
        }
    }
    while let Some(s) = pending.pop_front() {
        wait(gpu, s);
    }
    (started.elapsed().as_secs_f64(), measured)
}

/// コマ 10 の RGBA の FNV-1a ハッシュ。
fn hash(a: &Args, gpu: &Arc<GpuContext>) {
    let (mut dec, mut ring, conv) = setup(a, gpu);
    for n in 0..=10 {
        let f = next_frame(&mut dec);
        if n != 10 {
            continue;
        }
        let slot = ring.copy_frame(&f).expect("copy");
        ring.wait_copy_on(slot).expect("wait");
        conv.submit(gpu, slot);
        let rgba = conv.read_back(gpu).expect("read");
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in &rgba {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        println!("コマ 10 の RGBA ハッシュ {h:016x}");
        if let Ok(path) = std::env::var("SPIKE_DUMP") {
            std::fs::write(path, &rgba).expect("dump");
        }
    }
}

fn main() {
    let a = parse();
    let (layer, handle) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .expect("subscriber");
    let gpu = GpuContext::new().expect("gpu");
    println!("GPU: {}", gpu.adapter_name);
    if a.mode == "hash" {
        hash(&a, &gpu);
        return;
    }
    let (secs, n) = run(&a, &gpu);
    println!(
        "{} {} {n} コマ {secs:.3} s = {:.1} fps（inflight {}）",
        a.path.file_name().unwrap().to_string_lossy(),
        a.mode,
        n as f64 / secs,
        a.inflight
    );
    for (name, s) in handle.snapshot().spans {
        println!(
            "  {name:<16} n={:<5} p50 {:>7.3} ms  p95 {:>7.3} ms  max {:>7.3} ms",
            s.count,
            s.p50_us as f64 / 1000.0,
            s.p95_us as f64 / 1000.0,
            s.max_us as f64 / 1000.0
        );
    }
}
