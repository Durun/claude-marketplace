//! d3d12va の復号から wgpu のテクスチャまでの 1 コマの時間を、mm-trace のスパンで測る。
//!
//! 使い方: `cargo run --release --features d3d12va --example bench_d3d12va -- <素材> <mode> [--frames N] [--inflight K]`
//!
//! mode:
//! - `decode`: 復号だけ（wgpu に渡さない）。
//! - `zero`: 復号 → wgpu に包む → NV12 を RGBA に変換して完了まで待つ（コピーなし）。
//! - `cpu`: 復号 → `av_hwframe_transfer_data` → アップロード → 同じ変換（CPU 経由）。
//! - `verify`: 2 つの経路の同じコマを RGBA に変換して画素を比べる。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use mm_decode_api::Decoder;
use mm_decode_d3d12va::{Converter, CpuUploader, D3d12vaDecoder, GpuContext, HwDecoder, Wrapper};
use mm_trace::MetricsLayer;
use mm_types::{RationalTime, Size};
use tracing_subscriber::layer::SubscriberExt;

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

/// 完了を待つ前のコマ。GPU が読み終わるまで復号結果を握る。
struct InFlight {
    submission: wgpu::SubmissionIndex,
    _hw: ffmpeg_next::frame::Video,
}

fn wait(gpu: &GpuContext, f: InFlight) {
    let _s = tracing::info_span!("decode.gpu_wait").entered();
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(f.submission),
            timeout: None,
        })
        .expect("poll");
}

fn run(a: &Args, gpu: &Arc<GpuContext>) -> (f64, usize) {
    let mut dec = HwDecoder::open(&a.path, gpu).expect("open");
    let size = Size::new(dec.decoder.width(), dec.decoder.height());
    let conv = Converter::new(gpu, size);
    let up = CpuUploader::new(gpu, size);
    let mut wrapper = Wrapper::new(gpu.clone());
    let mut ring: VecDeque<InFlight> = VecDeque::new();
    let total = a.warmup + a.frames;
    let mut started = Instant::now();
    let mut measured = 0usize;
    for i in 0..total {
        if i == a.warmup {
            while let Some(f) = ring.pop_front() {
                wait(gpu, f);
            }
            started = Instant::now();
        }
        let frame_span = if i >= a.warmup {
            Some(tracing::info_span!("decode.frame", mode = a.mode.as_str()).entered())
        } else {
            None
        };
        let hw = {
            let _s = tracing::info_span!("decode.receive").entered();
            match dec.next_hw_frame().expect("decode") {
                Some(f) => f,
                None => {
                    dec.rewind().expect("rewind");
                    dec.next_hw_frame().expect("decode").expect("frame")
                }
            }
        };
        match a.mode.as_str() {
            "decode" => {
                drop(hw);
            }
            "zero" => {
                let nv12 = {
                    let _s = tracing::info_span!("decode.wrap").entered();
                    let n = wrapper.wrap(&hw).expect("wrap");
                    n.wait_decode_on(gpu).expect("wait");
                    n
                };
                let submission = {
                    let _s = tracing::info_span!("decode.convert").entered();
                    conv.submit(gpu, &nv12.y, &nv12.uv)
                };
                ring.push_back(InFlight {
                    submission,
                    _hw: hw,
                });
            }
            "cpu" => {
                let sw = {
                    let _s = tracing::info_span!("decode.transfer").entered();
                    HwDecoder::transfer_to_cpu(&hw).expect("transfer")
                };
                drop(hw);
                {
                    let _s = tracing::info_span!("decode.upload").entered();
                    up.upload(gpu, &sw);
                }
                let submission = {
                    let _s = tracing::info_span!("decode.convert").entered();
                    conv.submit(gpu, &up.y, &up.uv)
                };
                ring.push_back(InFlight {
                    submission,
                    _hw: sw,
                });
            }
            other => panic!("知らない mode {other}"),
        }
        while ring.len() > a.inflight {
            let f = ring.pop_front().expect("ring");
            wait(gpu, f);
        }
        drop(frame_span);
        if i >= a.warmup {
            measured += 1;
        }
    }
    while let Some(f) = ring.pop_front() {
        wait(gpu, f);
    }
    println!("包んだ資源の数: {}", wrapper.resource_count());
    (started.elapsed().as_secs_f64(), measured)
}

/// 同じコマを両方の経路で RGBA にして画素を比べる。
fn verify(a: &Args, gpu: &Arc<GpuContext>) {
    let mut dec = HwDecoder::open(&a.path, gpu).expect("open");
    let size = Size::new(dec.decoder.width(), dec.decoder.height());
    let conv = Converter::new(gpu, size);
    let up = CpuUploader::new(gpu, size);
    let mut wrapper = Wrapper::new(gpu.clone());
    let mut worst = 0u8;
    let mut checked = 0;
    for n in 0..150 {
        let hw = dec.next_hw_frame().expect("decode").expect("frame");
        if n % 7 != 3 {
            continue;
        }
        let nv12 = wrapper.wrap(&hw).expect("wrap");
        if std::env::var("SPIKE_NOWAIT").is_err() {
            nv12.wait_decode_on(gpu).expect("wait");
        }
        conv.submit(gpu, &nv12.y, &nv12.uv);
        let zero = conv.read_back(gpu).expect("read");
        let sw = HwDecoder::transfer_to_cpu(&hw).expect("transfer");
        up.upload(gpu, &sw);
        conv.submit(gpu, &up.y, &up.uv);
        let cpu = conv.read_back(gpu).expect("read");
        let (mut max, mut sum) = (0u8, 0u64);
        for (p, q) in zero.iter().zip(&cpu) {
            let d = p.abs_diff(*q);
            max = max.max(d);
            sum += u64::from(d);
        }
        let mean = sum as f64 / zero.len() as f64;
        let nonblack = zero.iter().filter(|v| **v > 16).count();
        if n == 10 {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in &zero {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            println!("コマ 10 の RGBA ハッシュ {h:016x}");
            if let Ok(path) = std::env::var("SPIKE_DUMP") {
                std::fs::write(path, &zero).expect("dump");
            }
        }
        worst = worst.max(max);
        checked += 1;
        if checked <= 3 {
            println!(
                "コマ {n}: 画素差 最大 {max} 平均 {mean:.4}（0 でない画素 {nonblack}/{}）",
                zero.len()
            );
        }
    }
    println!(
        "{checked} コマを比べた。画素差の最大 {worst}。包んだ資源 {}",
        wrapper.resource_count()
    );
}

/// 20 か所にシークし、頼んでからコマが wgpu のテクスチャとして使えるまでを測る。
fn seek_bench(a: &Args, gpu: &Arc<GpuContext>) {
    let mut dec = D3d12vaDecoder::open(&a.path, gpu.clone()).expect("open");
    let size = dec.info().size;
    let conv = Converter::new(gpu, size);
    let fps = dec.info().fps.expect("cfr");
    let count = dec.info().frame_count().max(1);
    for k in 0..40i64 {
        // 素数で散らす。
        let target = (k * 97 + 13) % (count - 5);
        let t = RationalTime::from_frames(target, fps).unwrap();
        let _s = tracing::info_span!("decode.seek").entered();
        dec.seek(t).expect("seek");
        let f = dec.next_frame().expect("next").expect("frame");
        if f.index != target {
            println!("シーク先 {target} に対して {} が出た", f.index);
        }
        let held = dec.take_frame(f.gpu.as_ref().expect("gpu")).expect("held");
        held.nv12.wait_decode_on(gpu).expect("wait");
        let sub = conv.submit(gpu, &held.nv12.y, &held.nv12.uv);
        let _ = gpu.device.poll(wgpu::PollType::Wait {
            submission_index: Some(sub),
            timeout: None,
        });
    }
}

fn main() {
    let a = parse();
    let (layer, handle) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .expect("subscriber");
    if std::env::var("SPIKE_LOG").is_ok() {
        ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Debug);
    }
    let gpu = GpuContext::new().expect("gpu");
    println!("GPU: {}", gpu.adapter_name);
    if a.mode == "verify" {
        verify(&a, &gpu);
        return;
    }
    if a.mode == "seek" {
        seek_bench(&a, &gpu);
        for (name, s) in handle.snapshot().spans {
            println!(
                "  {name:<18} n={:<4} p50 {:>7.3} ms  p95 {:>7.3} ms  max {:>7.3} ms",
                s.count,
                s.p50_us as f64 / 1000.0,
                s.p95_us as f64 / 1000.0,
                s.max_us as f64 / 1000.0
            );
        }
        return;
    }
    let (secs, n) = run(&a, &gpu);
    println!(
        "{} {} 本 {n} コマ {secs:.3} s = {:.1} fps（inflight {}）",
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
