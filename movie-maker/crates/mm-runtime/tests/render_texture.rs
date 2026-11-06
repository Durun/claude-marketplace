//! `Engine::render_frame_texture`: リードバックせずに返すテクスチャが、リードバックした画像と同じ画素を持つ。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::sync::Arc;

use mm_gpu::{WgpuGpu, FRAME_FORMAT};
use mm_node_api::Quality;
use mm_runtime::{assemble, Backend, Engine, RuntimeConfig};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::vertical_project;
use mm_types::{MmError, RationalTime, Size};

/// 1 画素のバイト数（`FRAME_FORMAT`）。
const BYTES_PER_PIXEL: u32 = 8;

fn wgpu_engine() -> Option<Engine> {
    let config = RuntimeConfig {
        render: Backend::Wgpu,
        ..RuntimeConfig::sample()
    };
    match assemble(&config) {
        Ok(e) => Some(e),
        Err(MmError::Unsupported(e)) if std::env::var_os("MM_REQUIRE_GPU").is_none() => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
        Err(e) => panic!("組めない: {e}"),
    }
}

#[test]
fn テクスチャのまま返したフレームはリードバックした画像と同じ() {
    let Some(mut engine) = wgpu_engine() else {
        return;
    };
    let media = SampleMedia::default();
    let path = std::env::temp_dir().join("mm_runtime_render_texture.json");
    let (project, comp, _) = vertical_project(&media, &path);
    let size = Size::new(media.size.width, media.size.height);
    let t = RationalTime::from_frames(3, media.fps).expect("表せる");
    let frame = engine
        .render_frame_texture(&project, &comp, t, size, Quality::Full)
        .expect("描ける");
    assert_eq!(frame.texture.format(), FRAME_FORMAT);
    assert_eq!(
        (frame.texture.width(), frame.texture.height()),
        (size.width, size.height)
    );
    let mut helper = WgpuGpu::new(Arc::clone(&frame.context));
    let readback = helper.begin_readback(&frame.texture, BYTES_PER_PIXEL);
    let from_texture = helper
        .finish_frame_readback(readback, size)
        .expect("読める");
    let expected = engine
        .render_frame(&project, &comp, t, size, Quality::Full)
        .expect("描ける");
    assert_eq!(from_texture.data(), expected.data());
}

#[test]
fn モックのレンダラーは_unsupported() {
    let media = SampleMedia::default();
    let path = std::env::temp_dir().join("mm_runtime_render_texture_sample.json");
    let (project, comp, _) = vertical_project(&media, &path);
    let mut engine = assemble(&RuntimeConfig::sample()).expect("組める");
    let size = Size::new(media.size.width, media.size.height);
    let got = engine.render_frame_texture(&project, &comp, RationalTime::ZERO, size, Quality::Full);
    assert!(matches!(got, Err(MmError::Unsupported(_))));
}
