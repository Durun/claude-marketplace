//! マクロを 1 つのトランザクションに展開する。

use std::collections::BTreeMap;

use mm_doc::{Interpolation, Keyframe, Layer, Node, Param};
use mm_ops::{Op, Transaction};
use mm_text_api::{TextAlign, TextShaper, TextStyle};
use mm_types::{MmError, ParamValue, RationalTime, Result};
use serde_json::{json, Value};

use crate::args::{millis, parse_time, Env};
use crate::layout::{place, Placement};
use crate::macros::{Step, TextStep};
use crate::pack::Pack;
use crate::sticker;
use crate::style::TextStyleSpec;

/// 文字の種類のバージョン（`mm-nodes-basic` の種類はどれも 1）。
const KIND_VERSION: u32 = 1;
/// プレートのレイヤーの ID に足す後ろの部分。
const PLATE_SUFFIX: &str = ".plate";

/// マクロ `name` を引数 `args`（名前 → 文字の値）で展開する。記録の名前は「<マクロの表示名>（<パック>）」。
/// プレートの大きさは `shaper` で文字を組んで決める（描くときと同じシェーパーを渡すと、プレートが文字にぴったり合う）。
/// 知らないマクロは `NotFound`、引数や手順の誤りは `InvalidArgument`。
pub fn expand(
    pack: &Pack,
    name: &str,
    args: &BTreeMap<String, String>,
    shaper: &dyn TextShaper,
) -> Result<Transaction> {
    let def = pack.find(name)?;
    let env = Env::new(def, &pack.theme, args)?;
    let mut ops = Vec::new();
    for (i, step) in def.steps.iter().enumerate() {
        let values = match step {
            Step::Op(raw) => vec![env.fill_json(raw)?],
            Step::Text(t) => text_ops(pack, &TextInput::read(pack, &env, t)?, shaper)?,
            Step::Mixer(m) => vec![mixer_op(pack, &env.fill_text(&m.comp)?)?],
            Step::Transition(t) => vec![crate::transition::transition_op(pack, &env, t)?],
            Step::Web(w) => vec![crate::web::web_op(pack, &env, w)?],
        };
        for value in values {
            let op: Op = serde_json::from_value(value).map_err(|e| {
                MmError::InvalidArgument(format!("{name} の手順 {}: 操作の形が違う: {e}", i + 1))
            })?;
            ops.push(op);
        }
    }
    let title = format!("{}（{}）", def.title, pack.manifest.name);
    Ok(Transaction::new(title, ops))
}

/// `mixer.json` を `comp` に当てる `setMixer`。パックに `mixer.json` が無ければ `InvalidArgument`。
fn mixer_op(pack: &Pack, comp: &str) -> Result<Value> {
    let mixer = pack.mixer.as_ref().ok_or_else(|| {
        MmError::InvalidArgument(format!(
            "パック {} に mixer.json が無い（mixer 手順が要る）",
            pack.manifest.name
        ))
    })?;
    Ok(json!({"op": "setMixer", "composition": comp, "mixer": serde_json::to_value(mixer)?}))
}

/// 引数を埋めて読んだ文字の手順。
struct TextInput<'a> {
    style: &'a TextStyleSpec,
    comp: String,
    id: String,
    text: String,
    at: RationalTime,
    dur: RationalTime,
    attach: Option<String>,
}

impl<'a> TextInput<'a> {
    fn read(pack: &'a Pack, env: &Env, step: &TextStep) -> Result<Self> {
        let style = pack.styles.get(&step.style).ok_or_else(|| {
            MmError::InvalidArgument(format!(
                "styles.json に文字のスタイル {} が無い",
                step.style
            ))
        })?;
        let id = env.fill_text(&step.layer)?;
        let mut text = env.fill_text(&step.text)?;
        if style.corrections {
            text = correct(&text, &pack.corrections);
        }
        let dur = parse_time(&env.fill_text(&step.dur)?)?;
        if dur <= RationalTime::ZERO {
            return Err(MmError::InvalidArgument(format!("{id}: 長さは正にする")));
        }
        let attach = match &step.attach {
            Some(raw) => Some(env.fill_text(raw)?).filter(|s| !s.is_empty()),
            None => None,
        };
        Ok(Self {
            style,
            comp: env.fill_text(&step.comp)?,
            id,
            text,
            at: parse_time(&env.fill_text(&step.at)?)?,
            dur,
            attach,
        })
    }
}

/// 文字のスタイルで文字を置く操作。プレートがあればプレートのレイヤーを下に、文字のレイヤーを上に足す。
fn text_ops(pack: &Pack, input: &TextInput<'_>, shaper: &dyn TextShaper) -> Result<Vec<Value>> {
    let style = input.style;
    let color = pack.theme.color(&style.color)?;
    let bounds = shaper.shape(&input.text, &text_style(style, color))?.bounds;
    let padding = style.plate.as_ref().map_or([0.0, 0.0], |p| p.padding);
    let block = [f64::from(bounds.width), f64::from(bounds.height)];
    let spot = place(block, padding, style.position, style.anchor);
    let frame = Frame {
        at: input.at,
        dur: input.dur,
        opacity: fade(style.fade_in, style.fade_out, input.at, input.dur)?,
        attach: input.attach.clone(),
        spot,
        tilt: style.tilt,
    };
    let mut layers = Vec::new();
    if let Some(plate) = &style.plate {
        let comp_size = [
            f64::from(pack.theme.canvas.width),
            f64::from(pack.theme.canvas.height),
        ];
        let mut node = Node::new("shader.wgsl", KIND_VERSION)
            .with_param("code", ParamValue::Text(sticker::code(&pack.theme, plate)?));
        for (name, value) in sticker::shape_params(plate, spot.plate, comp_size) {
            node = node.with_param(name, value);
        }
        layers.push(frame.layer(format!("{}{PLATE_SUFFIX}", input.id), node));
    }
    let node = text_node(style, &input.text, color, &spot);
    layers.push(frame.layer(input.id.clone(), node));
    layers
        .into_iter()
        .map(|layer| {
            let layer = serde_json::to_value(layer)?;
            Ok(json!({"op": "addLayer", "composition": input.comp, "layer": layer}))
        })
        .collect()
}

/// `text` の種類が描くときと同じスタイル（字の大きさ、色、折り返しの幅、中央揃え、行の高さの既定）。
fn text_style(style: &TextStyleSpec, color: mm_types::Rgba) -> TextStyle {
    TextStyle {
        size_px: style.size as f32,
        color,
        max_width: style.max_width.map(|w| w as f32),
        align: TextAlign::Center,
        ..TextStyle::default()
    }
}

/// プレートと文字のレイヤーが共有する時間と置き方。
struct Frame {
    at: RationalTime,
    dur: RationalTime,
    opacity: Param,
    attach: Option<String>,
    spot: Placement,
    tilt: f64,
}

impl Frame {
    fn layer(&self, id: String, node: Node) -> Layer {
        let mut layer = Layer::new(id, node, self.at, RationalTime::ZERO, self.dur);
        layer.opacity = self.opacity.clone();
        layer.attach_to = self.attach.as_deref().map(mm_types::Id::new);
        if self.tilt != 0.0 {
            // 回転の中心はコンポジションの画素で、プレートの中心。プレートと文字が同じ中心で回るので重なりがずれない。
            let [x, y, w, h] = self.spot.plate;
            layer.transform.rotation = Param::constant(ParamValue::Number(self.tilt));
            layer.transform.anchor = Some(Param::constant(ParamValue::Vec2([
                x + w / 2.0,
                y + h / 2.0,
            ])));
        }
        layer
    }
}

fn text_node(style: &TextStyleSpec, text: &str, color: mm_types::Rgba, spot: &Placement) -> Node {
    Node::new("text", KIND_VERSION)
        .with_param("text", ParamValue::Text(text.to_string()))
        .with_param("size", ParamValue::Number(style.size))
        .with_param("color", ParamValue::Color(color))
        .with_param("position", ParamValue::Vec2(spot.text_origin))
        .with_param(
            "max_width",
            ParamValue::Number(style.max_width.unwrap_or(0.0)),
        )
        .with_param("align", ParamValue::Text("center".into()))
}

/// 出る・消える動き（秒）の不透明度。動きの長さの和が出ている長さを超えるときは動かさない。
pub(crate) fn fade(
    fade_in: f64,
    fade_out: f64,
    at: RationalTime,
    dur: RationalTime,
) -> Result<Param> {
    let constant = Param::constant(ParamValue::Number(1.0));
    if fade_in <= 0.0 && fade_out <= 0.0 {
        return Ok(constant);
    }
    let fade_in = millis(fade_in)?;
    let fade_out = millis(fade_out)?;
    if fade_in.checked_add(fade_out)? > dur {
        return Ok(constant);
    }
    let end = at.checked_add(dur)?;
    let key =
        |t: RationalTime, v: f64| Keyframe::new(t, ParamValue::Number(v), Interpolation::Linear);
    let mut keys = vec![
        key(
            at,
            if fade_in > RationalTime::ZERO {
                0.0
            } else {
                1.0
            },
        ),
        key(at.checked_add(fade_in)?, 1.0),
        key(end.checked_sub(fade_out)?, 1.0),
        key(
            end,
            if fade_out > RationalTime::ZERO {
                0.0
            } else {
                1.0
            },
        ),
    ];
    keys.dedup_by(|a, b| a.time == b.time);
    Ok(Param {
        value: ParamValue::Number(1.0),
        keyframes: keys,
    })
}

/// 表記の置き換え。先頭から 1 回だけ走査し、各位置でいちばん長い語を置き換える。
/// 置き換えた後の文字は走査し直さないので、ある語の置き換え先が別の語にまた置き換わることはない。
fn correct(text: &str, corrections: &BTreeMap<String, String>) -> String {
    let mut pairs: Vec<(&str, &str)> = corrections
        .iter()
        .filter(|(from, _)| !from.is_empty())
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect();
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        match pairs.iter().find(|(from, _)| rest.starts_with(from)) {
            Some((from, to)) => {
                out.push_str(to);
                rest = &rest[from.len()..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn 長い語から置き換える() {
        let t = table(&[("ボーカロイド6", "VOCALOID6"), ("ボーカロイド", "VOCALOID")]);
        assert_eq!(
            correct("ボーカロイド6とボーカロイド", &t),
            "VOCALOID6とVOCALOID"
        );
    }

    #[test]
    fn 置き換えた後の文字は置き換え直さない() {
        let t = table(&[("クロード", "Claude"), ("Cl", "CL")]);
        assert_eq!(correct("クロードとCl", &t), "ClaudeとCL");
    }
}
