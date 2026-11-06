//! CPU の RGBA への変換と、YUV のまま GPU に上げる経路の、デコーダー単体の速さを比べる（feature `gpu`）。
//!
//! `cargo run -p mm-decode-ffmpeg --release --example bench_upload --features gpu -- <素材> [フレーム数]`

use std::path::PathBuf;
use std::time::Instant;

use mm_decode_api::DecoderFactory;
use mm_decode_ffmpeg::{FfmpegCpuDecoderFactory, FfmpegDecoderFactory};
use mm_gpu::{GpuContext, GpuRequest};
use mm_types::Result;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().expect("素材のパス"));
    let frames: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(60);
    let ctx = GpuContext::shared(&GpuRequest::default())?;
    let factories: [(&str, Box<dyn DecoderFactory>); 2] = [
        ("cpu rgba f32", Box::new(FfmpegCpuDecoderFactory)),
        ("gpu yuv upload", Box::new(FfmpegDecoderFactory)),
    ];
    for (name, factory) in factories {
        let mut dec = factory.open(&path, Some(ctx.as_ref()))?;
        // 最初のフレームは開く費用が混ざるので数えない。
        dec.next_frame()?;
        let started = Instant::now();
        let mut n = 0;
        while n < frames && dec.next_frame()?.is_some() {
            n += 1;
        }
        // GPU の経路は提出した変換の完了まで待って数える。
        let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
        let secs = started.elapsed().as_secs_f64();
        println!(
            "{name}: {n} frames in {:.3} s = {:.1} fps ({:.2} ms/frame)",
            secs,
            n as f64 / secs,
            secs * 1000.0 / n as f64
        );
    }
    Ok(())
}
