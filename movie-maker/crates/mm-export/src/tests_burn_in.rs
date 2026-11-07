//! 字幕の焼き込み。字幕のレイヤーが字幕の時間と置き場所に合うことと、書き出しの絵に字幕が出る時刻が
//! `composition_cues` の結果に一致することを確かめる。

use super::*;
use crate::burn_in::{burn_in_captions, burn_in_enabled};
use crate::captions::{composition_cues, CaptionWord};
use mm_doc::{
    CaptionPlacement, Composition, Cut, Layer, Node, Param, SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_node_api::Quality;
use mm_runtime::{assemble, Backend, RuntimeConfig};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::vertical_project;
use mm_types::{CompositionId, Id, ParamValue, Rational, RationalTime, RgbaImage, Size};
use std::sync::Arc;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const FPS: i64 = 30;
/// 画素の差とみなす 1 チャンネルの違い。
const PIXEL_EPSILON: f32 = 1e-3;

fn t(n: i64, d: u32) -> RationalTime {
    RationalTime::new(i128::from(n), d).unwrap()
}

fn word(start: (i64, u32), end: (i64, u32), text: &str) -> CaptionWord {
    CaptionWord {
        start: t(start.0, start.1),
        end: t(end.0, end.1),
        text: text.into(),
    }
}

/// 素材 clip の 3 つの文。間は離れているので字幕 3 件になる。
fn transcript() -> TranscriptWords {
    let mut m = TranscriptWords::new();
    m.insert(
        Id::new("clip"),
        vec![
            word((0, 1), (1, 2), "こんにちは。"),
            word((2, 1), (13, 5), "二番目の字幕です。"),
            word((5, 1), (27, 5), "三つ目。"),
        ],
    );
    m
}

fn media() -> SampleMedia {
    SampleMedia {
        size: Size::new(WIDTH, HEIGHT),
        ..SampleMedia::default()
    }
}

/// 素材 clip の 0〜8 秒を 1 本の sequence にして並べたコンポジションを持つプロジェクト。
fn project() -> (Project, CompositionId) {
    let (mut p, comp_id, _) = vertical_project(&media(), &std::env::temp_dir().join("burn.json"));
    let footage = Node::footage(&Id::new("clip"));
    p.sequences.push(mm_doc::Sequence {
        id: Id::new("s"),
        cuts: vec![Cut::new("k", footage, t(0, 1), t(8, 1))],
        origin: None,
    });
    let node = Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text("s".into()));
    let layer = Layer::new("seq", node, RationalTime::ZERO, RationalTime::ZERO, t(8, 1));
    let comp: &mut Composition = p.composition_mut(&comp_id).unwrap();
    comp.duration = t(8, 1);
    comp.layers = vec![layer];
    p.render_targets[0].delivery.burn_in_captions = true;
    (p, comp_id)
}

fn shaper() -> std::sync::Arc<dyn mm_text_api::TextShaper> {
    let engine = assemble(&RuntimeConfig::sample()).unwrap();
    engine.services().text_shaper.clone()
}

fn burned(p: &Project, id: &CompositionId) -> Project {
    burn_in_captions(p, id, &transcript(), shaper().as_ref()).unwrap()
}

fn caption_layers(p: &Project, id: &CompositionId) -> Vec<Layer> {
    let layers = &p.composition(id).unwrap().layers;
    layers
        .iter()
        .filter(|l| l.source.kind == "text")
        .cloned()
        .collect()
}

fn position_of(layer: &Layer) -> [f64; 2] {
    layer.source.params["position"].value.as_vec2().unwrap()
}

#[test]
fn 焼き込む設定のコンポジションだけ有効になる() {
    let (mut p, id) = project();
    assert!(burn_in_enabled(&p, &id));
    p.render_targets[0].delivery.burn_in_captions = false;
    assert!(!burn_in_enabled(&p, &id));
}

#[test]
fn 字幕の件数と時間に合う_影と本体のレイヤーが足される() {
    let (p, id) = project();
    let cues = composition_cues(
        &p,
        p.composition(&id).unwrap(),
        p.composition(&id).unwrap().range().unwrap(),
        &transcript(),
    )
    .unwrap();
    assert_eq!(cues.len(), 3);
    let layers = caption_layers(&burned(&p, &id), &id);
    assert_eq!(layers.len(), cues.len() * 2);
    for (cue, pair) in cues.iter().zip(layers.chunks(2)) {
        for layer in pair {
            assert_eq!(layer.start, cue.start);
            assert_eq!(layer.range().unwrap().end, cue.end);
            assert_eq!(layer.source.text_param("text"), Some(cue.text.as_str()));
        }
        let color = |l: &Layer| l.source.params["color"].value.as_color().unwrap();
        assert_eq!(color(&pair[0]), mm_types::Rgba::BLACK, "影が下");
        assert_eq!(color(&pair[1]), mm_types::Rgba::WHITE, "本体が上");
    }
}

#[test]
fn 置き場所が無ければ下部中央_あればその位置と幅に置く() {
    let (p, id) = project();
    let default = caption_layers(&burned(&p, &id), &id);
    let text = &default[1];
    let width = text.source.params["max_width"].value.as_number().unwrap();
    assert_eq!(width, f64::from(WIDTH) * 0.9);
    let [x, y] = position_of(text);
    assert!(
        x > 0.0 && x < f64::from(WIDTH) / 2.0,
        "左上は中央より左: {x}"
    );
    assert!(y > f64::from(HEIGHT) / 2.0, "下半分に置く: {y}");

    let mut set = p.clone();
    set.composition_mut(&id).unwrap().layers[0].framing.caption = Some(CaptionPlacement {
        position: Param::constant(ParamValue::Vec2([300.0, 200.0])),
        width: Param::constant(ParamValue::Number(400.0)),
    });
    let placed = caption_layers(&burned(&set, &id), &id);
    let text = &placed[1];
    assert_eq!(
        text.source.params["max_width"].value.as_number(),
        Some(400.0)
    );
    let [x, y] = position_of(text);
    assert!(y < 200.0, "下端が 200 なので左上はそれより上: {y}");
    assert!(x < 300.0, "中央が 300 なので左上はそれより左: {x}");
}

#[test]
fn 字幕にできる文字起こしが無ければレイヤーを足さない() {
    let (p, id) = project();
    let none = burn_in_captions(&p, &id, &TranscriptWords::new(), shaper().as_ref()).unwrap();
    assert_eq!(none, p);
}

#[test]
fn 焼き込む設定で文字起こしが無いと書き出しを拒否する() {
    let (mut p, _) = project();
    p.render_targets[0].output_path = temp("burn_nosub").join("out.json").to_string_lossy().into();
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let err = export_project(&mut engine, &p, &ExportOptions::default()).unwrap_err();
    assert!(matches!(err, MmError::InvalidArgument(_)), "{err}");
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mm_export_{name}_{}", std::process::id()))
}

/// sequence を展開できる wgpu のレンダラーと、モックのデコーダー。GPU が無ければ `None`
/// （`MM_REQUIRE_GPU` があれば失敗にする）。
fn gpu_engine() -> Option<mm_runtime::Engine> {
    let config = RuntimeConfig {
        render: Backend::Wgpu,
        decode: Backend::Sample,
        encode: Backend::Sample,
        ..RuntimeConfig::real()
    };
    match assemble(&config) {
        Ok(engine) => Some(engine),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(_) => None,
    }
}

fn frame(
    engine: &mut mm_runtime::Engine,
    p: &Project,
    id: &CompositionId,
    index: i64,
) -> Arc<RgbaImage> {
    let time = RationalTime::from_frames(index, Rational::integer(FPS as u32)).unwrap();
    engine
        .render_frame(p, id, time, Size::new(WIDTH, HEIGHT), Quality::Full)
        .unwrap()
}

/// 2 枚の絵で違う画素の、縦の範囲 `(最小の y, 最大の y)`。同じなら `None`。
fn diff_rows(a: &Arc<RgbaImage>, b: &Arc<RgbaImage>) -> Option<(u32, u32)> {
    let mut rows: Option<(u32, u32)> = None;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            let same = (p.r - q.r).abs() < PIXEL_EPSILON
                && (p.g - q.g).abs() < PIXEL_EPSILON
                && (p.b - q.b).abs() < PIXEL_EPSILON;
            if !same {
                rows = Some(rows.map_or((y, y), |(lo, hi)| (lo.min(y), hi.max(y))));
            }
        }
    }
    rows
}

/// 字幕の境界の前後と中ほどのフレーム（重ならない 3 件の字幕のそれぞれ）。
fn probe_frames(cues: &[mm_decode_api::SubtitleCue]) -> Vec<i64> {
    let fps = Rational::integer(FPS as u32);
    let first_at_or_after = |time: RationalTime| {
        (0..)
            .find(|k| RationalTime::from_frames(*k, fps).unwrap() >= time)
            .unwrap()
    };
    let mut frames = vec![0, 299];
    for cue in cues {
        let (start, end) = (first_at_or_after(cue.start), first_at_or_after(cue.end));
        frames.extend([start - 1, start, (start + end) / 2, end - 1, end]);
    }
    frames.retain(|f| (0..8 * FPS).contains(f));
    frames
}

#[test]
fn 字幕が絵に出る時刻は字幕の時間と一致し_位置は下部中央() {
    let Some(mut engine) = gpu_engine() else {
        return;
    };
    let (p, id) = project();
    let comp = p.composition(&id).unwrap();
    let cues = composition_cues(&p, comp, comp.range().unwrap(), &transcript()).unwrap();
    let with = burned(&p, &id);
    let fps = Rational::integer(FPS as u32);
    for index in probe_frames(&cues) {
        let time = RationalTime::from_frames(index, fps).unwrap();
        let expected = cues.iter().any(|c| c.start <= time && time < c.end);
        let rows = diff_rows(
            &frame(&mut engine, &p, &id, index),
            &frame(&mut engine, &with, &id, index),
        );
        assert_eq!(
            rows.is_some(),
            expected,
            "フレーム {index}（{} 秒）の字幕の有無",
            time.to_f64()
        );
        if let Some((top, bottom)) = rows {
            assert!(
                top > HEIGHT / 2 && bottom < HEIGHT,
                "下部に描く: {top}..{bottom}"
            );
        }
    }
}
