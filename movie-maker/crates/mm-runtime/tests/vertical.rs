//! 縦の 1 本: 素材 1 本を切って、文字を載せ、音を付けるまでを `Engine` 経由で通す。
//! 書き出しまでの一本は `mm-export` のテストが持つ。全部がモックの実装。

use mm_node_api::Quality;
use mm_runtime::{assemble, RuntimeConfig};
use mm_testkit::decode::{frame_index_of, SampleMedia};
use mm_testkit::fixtures::{vertical_project, VERTICAL_DURATION_SECONDS, VERTICAL_IN_SECONDS};
use mm_types::{RationalTime, Size};

fn output_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mm_runtime_vertical_{name}.json"))
}

#[test]
fn 切った素材のフレームが_in_点からの番号で出る() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &output_path("frame"));
    let mut engine = assemble(&RuntimeConfig::sample()).expect("組める");
    let size = Size::new(media.size.width, media.size.height);

    for frame in [0i64, 1, 29, 59] {
        let t = RationalTime::from_frames(frame, media.fps).expect("表せる");
        let img = engine
            .render_frame(&project, &comp, t, size, Quality::Full)
            .expect("描ける");
        let expected = VERTICAL_IN_SECONDS * media.fps.num() as i64 + frame;
        assert_eq!(
            frame_index_of(img.pixel(0, 0)),
            expected,
            "フレーム {frame} の素材の番号"
        );
    }
}

#[test]
fn 文字が素材の上に載る() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &output_path("text"));
    let mut engine = assemble(&RuntimeConfig::sample()).expect("組める");
    let size = Size::new(media.size.width, media.size.height);
    let img = engine
        .render_frame(&project, &comp, RationalTime::ZERO, size, Quality::Full)
        .expect("描ける");
    // 文字は (16, 120) に 24 px で置くので、その中の画素は白。
    let on_text = img.pixel(20, 130);
    assert!(
        on_text.r > 0.99 && on_text.g > 0.99 && on_text.b > 0.99,
        "文字の上は白: {on_text:?}"
    );
    let off_text = img.pixel(0, 0);
    assert!(off_text.r < 0.5, "文字の外は素材の色: {off_text:?}");
}

#[test]
fn 音は無音でなく長さが合う() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &output_path("audio"));
    let mut engine = assemble(&RuntimeConfig::sample()).expect("組める");
    let range = project.composition(&comp).unwrap().range().unwrap();
    let audio = engine
        .render_audio(&project, &comp, range)
        .expect("音が出る");
    assert_eq!(
        audio.frames(),
        VERTICAL_DURATION_SECONDS as usize * mm_audio_api::DEFAULT_SAMPLE_RATE as usize
    );
    assert!(!audio.is_silent(), "素材の音が乗っている");
}

#[test]
fn 選んだ実装の名前が見える() {
    let engine = assemble(&RuntimeConfig::sample()).expect("組める");
    for (_, name) in engine.backend_names() {
        assert!(name == "sample" || name == "memory", "{name}");
    }
    assert_eq!(engine.registry().len(), 3);
}
