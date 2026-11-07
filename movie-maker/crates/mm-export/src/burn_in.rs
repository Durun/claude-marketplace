//! 字幕の焼き込み。字幕の 1 件ごとに text ノードのレイヤーを足したプロジェクトのデータを作る。
//!
//! 絵を描くのはレンダラーの text ノードなので、書き出しもプレビューも同じ関数が返すプロジェクトのデータを描けば、
//! 字幕の時刻・位置・大きさが一致する。字幕の時刻は [`composition_cues`]（SRT などと同じ元）。

use mm_decode_api::SubtitleCue;
use mm_doc::{Composition, Layer, Node, Project, SEQUENCE_KIND};
use mm_text_api::{TextAlign, TextShaper, TextStyle};
use mm_types::{CompositionId, ParamValue, RationalTime, Result, Rgba};

use crate::captions::{composition_cues, TranscriptWords};

/// 字幕の字の大きさ。コンポジションの短い辺に対する比。
const SIZE_RATIO: f64 = 0.045;
/// 既定の帯の幅。コンポジションの幅に対する比。
const DEFAULT_WIDTH_RATIO: f64 = 0.9;
/// 既定の帯の下端の、コンポジションの下からの距離。高さに対する比。
const DEFAULT_BOTTOM_MARGIN_RATIO: f64 = 0.08;
/// 影の字の色。明るい絵の上でも読めるよう、同じ文字を黒で少しずらして下に敷く。
const SHADOW_COLOR: Rgba = Rgba::BLACK;
/// 影のずらしの大きさ。字の大きさに対する比。
const SHADOW_OFFSET_RATIO: f64 = 1.0 / 16.0;
/// 影のずらしの下限（画素）。小さな字でも影が字に重なりきらないようにする。
const MIN_SHADOW_OFFSET_PX: f64 = 1.0;
/// text ノードのバージョン。
const TEXT_KIND_VERSION: u32 = 1;
/// 足すレイヤーの ID の接頭辞。
const LAYER_PREFIX: &str = "burn-in-caption";

/// 字幕の置き場所と大きさ（コンポジションの画素）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    /// 帯の下端の中央。
    bottom_center: [f64; 2],
    /// 帯の幅。
    width: f64,
    /// 字の大きさ。
    size: f64,
}

/// どれかのレンダーターゲットが、このコンポジションの字幕を焼き込むか。
pub fn burn_in_enabled(project: &Project, composition: &CompositionId) -> bool {
    project
        .render_targets
        .iter()
        .any(|t| &t.composition == composition && t.delivery.burn_in_captions)
}

/// コンポジション `composition` の字幕を焼き込んだプロジェクトのデータ。字幕は全長から作るので、書き出す区間によらず
/// 同じ時刻に同じ字幕が出る。字幕にできる文字起こしが無ければ、レイヤーは足さない。
pub fn burn_in_captions(
    project: &Project,
    composition: &CompositionId,
    transcript: &TranscriptWords,
    shaper: &dyn TextShaper,
) -> Result<Project> {
    let comp = project.composition(composition)?;
    let cues = composition_cues(project, comp, comp.range()?, transcript)?;
    let placement = placement_of(comp);
    let layers = caption_layers(&cues, placement, shaper)?;
    let mut out = project.clone();
    out.composition_mut(composition)?.layers.extend(layers);
    Ok(out)
}

/// 最初の、置き場所を持つ有効な sequence のレイヤーの `setCaption`。無ければ下部中央。
fn placement_of(comp: &Composition) -> Placement {
    let size = f64::from(comp.width.min(comp.height)) * SIZE_RATIO;
    let set = comp
        .layers
        .iter()
        .filter(|l| l.enabled && l.source.kind == SEQUENCE_KIND)
        .find_map(|l| l.framing.caption.as_ref());
    let (bottom_center, width) = match set {
        Some(c) => (
            c.position
                .value
                .as_vec2()
                .unwrap_or_else(|| default_bottom(comp)),
            c.width
                .value
                .as_number()
                .unwrap_or_else(|| default_width(comp)),
        ),
        None => (default_bottom(comp), default_width(comp)),
    };
    Placement {
        bottom_center,
        width,
        size,
    }
}

fn default_bottom(comp: &Composition) -> [f64; 2] {
    let h = f64::from(comp.height);
    [
        f64::from(comp.width) / 2.0,
        h - h * DEFAULT_BOTTOM_MARGIN_RATIO,
    ]
}

fn default_width(comp: &Composition) -> f64 {
    f64::from(comp.width) * DEFAULT_WIDTH_RATIO
}

/// 字幕 1 件につき、影と本体の text ノードのレイヤーを 2 つ（影が下）。時間は字幕の始まりから終わりまで。
fn caption_layers(
    cues: &[SubtitleCue],
    placement: Placement,
    shaper: &dyn TextShaper,
) -> Result<Vec<Layer>> {
    let mut layers = Vec::new();
    let offset = (placement.size * SHADOW_OFFSET_RATIO).max(MIN_SHADOW_OFFSET_PX);
    for (i, cue) in cues.iter().enumerate().filter(|(_, c)| c.end > c.start) {
        let origin = origin_of(shaper, &cue.text, placement)?;
        let duration = cue.end.checked_sub(cue.start)?;
        for (suffix, color, shift) in [("shadow", SHADOW_COLOR, offset), ("text", Rgba::WHITE, 0.0)]
        {
            let node = text_node(
                &cue.text,
                placement,
                color,
                [origin[0] + shift, origin[1] + shift],
            );
            layers.push(Layer::new(
                format!("{LAYER_PREFIX}-{i}-{suffix}"),
                node,
                cue.start,
                RationalTime::ZERO,
                duration,
            ));
        }
    }
    Ok(layers)
}

/// text ノードのスタイル。ノードの種類が使うのと同じ値（折り返しの幅、中央揃え、行の高さの既定）。
fn style_of(placement: Placement, color: Rgba) -> TextStyle {
    TextStyle {
        size_px: placement.size as f32,
        color,
        max_width: Some(placement.width as f32),
        align: TextAlign::Center,
        ..TextStyle::default()
    }
}

/// 帯の下端の中央に、組んだ字の塊の下端の中央を合わせたときの左上。
fn origin_of(shaper: &dyn TextShaper, text: &str, placement: Placement) -> Result<[f64; 2]> {
    let shaped = shaper.shape(text, &style_of(placement, Rgba::WHITE))?;
    let [cx, bottom] = placement.bottom_center;
    Ok([
        cx - f64::from(shaped.bounds.width) / 2.0,
        bottom - f64::from(shaped.bounds.height),
    ])
}

fn text_node(text: &str, placement: Placement, color: Rgba, origin: [f64; 2]) -> Node {
    Node::new("text", TEXT_KIND_VERSION)
        .with_param("text", ParamValue::Text(text.into()))
        .with_param("size", ParamValue::Number(placement.size))
        .with_param("color", ParamValue::Color(color))
        .with_param("position", ParamValue::Vec2(origin))
        .with_param("max_width", ParamValue::Number(placement.width))
        .with_param("align", ParamValue::Text("center".into()))
}
