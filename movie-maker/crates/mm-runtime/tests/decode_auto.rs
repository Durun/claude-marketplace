//! GPU でデコードする分岐（`d3d12va`、`cuvid`、`auto`）を、実機の GPU とフィクスチャの素材で `Engine` のインターフェースまで通す。
//! 実機の NVIDIA と FFmpeg が要るので、3 つの feature がそろったときだけ走る。
//!
//! レンダラーの `footage` の種類はまだ GPU のフレームを受けない（段階 2）ので、ここでは `Engine::decoders` の
//! インターフェースでフレームを開き、レンダラーと同じ手順（`GpuHelper::import`）でリードバックして番号を確かめる。
//! 共有の GPU を D3D12 で頼むので、既定の頼み方で共有の GPU を作るほかのテストと同じプロセスに置かない（別の実行ファイル。docs/ci.md）。
#![cfg(all(feature = "ffmpeg", feature = "d3d12va", feature = "cuvid"))]

use std::path::PathBuf;
use std::sync::Arc;

use mm_decode_api::{DecodedFrame, DecoderFactory, GpuContextRef};
use mm_gpu::{GpuContext, WgpuGpu};
use mm_node_api::GpuHelper;
use mm_runtime::{assemble, AutoDecoderFactory, Backend, GpuCandidate, RuntimeConfig, WithAudio};
use mm_types::{RationalTime, RgbaImage, TimeRange};

const BIT_COUNT: u32 = 16;
const BIT_SIZE: u32 = 16;

fn fixtures() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/media");
    let out = root.join("out");
    if !out.join("cfr.mp4").exists() {
        let status = std::process::Command::new("sh")
            .arg(root.join("generate.sh"))
            .status()
            .expect("sh");
        assert!(status.success(), "generate.sh");
    }
    out
}

fn number(image: &RgbaImage, scale: u32) -> i64 {
    (0..BIT_COUNT)
        .map(|k| {
            let x = (k * BIT_SIZE + BIT_SIZE / 2) * scale;
            ((image.pixel(x, BIT_SIZE / 2 * scale).r > 0.5) as i64) << k
        })
        .sum()
}

fn config(decode: Backend) -> RuntimeConfig {
    RuntimeConfig {
        render: Backend::Wgpu,
        decode,
        audio: Backend::Dsp,
        text: Backend::Glyphon,
        ..RuntimeConfig::sample()
    }
}

/// `assemble` が作ったのと同じ GPU のコンテキスト（プロセスで共有）。
fn shared_gpu() -> Arc<GpuContext> {
    GpuContext::shared(&mm_decode_d3d12va::gpu_request(None).unwrap()).unwrap()
}

/// フレームの画像を CPU に読み戻す。CPU のフレームはそのまま。
fn pixels(helper: &mut WgpuGpu, f: &DecodedFrame) -> RgbaImage {
    if let Some(image) = &f.image {
        return image.clone();
    }
    let frame = helper
        .import(f.gpu.as_ref().expect("GPU のフレーム"))
        .unwrap();
    helper.before_submit();
    let image = helper.download(&frame).unwrap();
    helper.after_submit();
    (*image).clone()
}

/// `Engine` のインターフェースで `cfr.mp4` を開き、フレーム 0、1、17、29 の焼き込んだ番号を読む。
fn engine_numbers(decode: Backend) -> Vec<i64> {
    let engine = assemble(&config(decode)).expect("組める");
    let ctx = shared_gpu();
    let mut helper = WgpuGpu::new(ctx.clone());
    let mut dec = engine
        .decoders()
        .open(
            &fixtures().join("cfr.mp4"),
            Some(ctx.as_ref() as &dyn GpuContextRef),
        )
        .expect("開ける");
    assert!(dec.info().audio.is_some(), "音の情報は ffmpeg から来る");
    let fps = dec.info().fps.unwrap();
    [0i64, 1, 17, 29]
        .iter()
        .map(|i| {
            dec.seek(RationalTime::from_frames(*i, fps).unwrap())
                .unwrap();
            let f = dec.next_frame().unwrap().unwrap();
            assert_eq!(f.index, *i);
            number(&pixels(&mut helper, &f), 1)
        })
        .collect()
}

#[test]
fn auto_で開いたフレームの番号が合う() {
    assert_eq!(engine_numbers(Backend::Auto), vec![0, 1, 17, 29]);
}

#[test]
fn d3d12va_を選んで開ける() {
    assert_eq!(engine_numbers(Backend::D3d12va), vec![0, 1, 17, 29]);
}

#[test]
fn cuvid_を選んで開ける() {
    assert_eq!(engine_numbers(Backend::Cuvid), vec![0, 1, 17, 29]);
}

#[test]
fn 選んだ名前が_engine_に出る() {
    let engine = assemble(&config(Backend::Auto)).expect("組める");
    assert!(engine.backend_names().contains(&("decode", "auto")));
}

#[test]
fn 素材ごとに実装を選び_音も付く() {
    let ctx = shared_gpu();
    let ffmpeg: Arc<dyn DecoderFactory> = Arc::new(mm_decode_ffmpeg::FfmpegDecoderFactory);
    let with_audio = |f: Arc<dyn DecoderFactory>| -> Arc<dyn DecoderFactory> {
        Arc::new(WithAudio::new(f, ffmpeg.clone()))
    };
    let auto = AutoDecoderFactory::new(
        vec![
            GpuCandidate::probed_on_open(with_audio(Arc::new(mm_decode_d3d12va::D3d12vaFactory))),
            GpuCandidate::new(with_audio(Arc::new(mm_decode_cuvid::CuvidFactory))),
        ],
        Some(ffmpeg.clone()),
    );
    let gpu = Some(ctx.as_ref() as &dyn GpuContextRef);

    let (mut dec, chosen) = auto
        .open_with_choice(&fixtures().join("cfr.mp4"), gpu)
        .expect("開ける");
    assert_eq!(chosen, "d3d12va");
    let range = TimeRange::new(RationalTime::ZERO, RationalTime::seconds(1)).unwrap();
    assert!(
        !dec.decode_audio(range).expect("音").is_silent(),
        "無音でない"
    );

    let (_, chosen) = auto
        .open_with_choice(&fixtures().join("cfr.mp4"), None)
        .expect("開ける");
    assert_eq!(chosen, "ffmpeg", "GPU のコンテキストが無ければ CPU");
}

/// NVENC の 4K HEVC は d3d12va がデコードに失敗する環境がある。cuvid か d3d12va が開き、番号は合う。
#[test]
fn nvenc_の_4k_hevc_は_gpu_の実装で開ける() {
    let nvenc = fixtures().join("nvenc4k.mp4");
    if !nvenc.exists() {
        eprintln!("nvenc4k.mp4 が無いのでスキップする");
        return;
    }
    let ctx = shared_gpu();
    let mut helper = WgpuGpu::new(ctx.clone());
    let ffmpeg: Arc<dyn DecoderFactory> = Arc::new(mm_decode_ffmpeg::FfmpegDecoderFactory);
    let auto = AutoDecoderFactory::new(
        vec![
            GpuCandidate::probed_on_open(Arc::new(mm_decode_d3d12va::D3d12vaFactory)),
            GpuCandidate::new(Arc::new(mm_decode_cuvid::CuvidFactory)),
        ],
        Some(ffmpeg),
    );
    let (mut dec, chosen) = auto
        .open_with_choice(&nvenc, Some(ctx.as_ref() as &dyn GpuContextRef))
        .expect("開ける");
    eprintln!("nvenc4k.mp4 は {chosen} で開いた");
    assert_ne!(chosen, "ffmpeg", "GPU の実装のどちらかで開く");
    for i in 0..3 {
        let f = dec.next_frame().expect("デコード").expect("フレーム");
        assert_eq!(number(&pixels(&mut helper, &f), 12), i);
    }
}
