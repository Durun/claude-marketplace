//! 映像と画像のレイヤー（`layers[].clips[]` の `video` と `image`）を movie-maker のレイヤーにする。
//!
//! geneva はクリップの矩形（切り抜いて `fit` で収めたもの）のアンカーを `position` に置き、その周りで拡大する。
//! movie-maker では素材を `fit: none`（素材の画素のまま左上に置く）で描き、アンカーを原点に固定して、
//! 同じ対応になる拡大と位置を求める。レイヤーの座標は素材の画素なので、切り抜きはレイヤーの切り抜き（`crop`）、
//! 角丸の矩形のマスクはレイヤーのマスクに、クリップの矩形の画素を素材の画素に戻して置く。

use std::collections::HashMap;

use mm_doc::{Interpolation, Keyframe, Layer, LayerAudio, Node, Param, Transform};
use mm_types::{Id, MmError, ParamValue, Rational, RationalTime, Result};
use serde_json::Value;

use super::media::Media;
use super::report::{Gap, Report};
use super::shape;
use super::syntax::{self, Anim};

/// 素材の画素のまま置く `fit` のパラメータ（`mm-nodes-basic` の footage と image）。
pub const FIT_PARAM: &str = "fit";
/// `fit` の値。
pub const FIT_NONE: &str = "none";
/// 撮影素材と静止画のノードの種類と、素材のパラメータ。
const FOOTAGE_KIND: &str = "footage";
const IMAGE_KIND: &str = "image";
const ASSET_PARAM: &str = "asset";
const KIND_VERSION: u32 = 1;

/// 変換の共通の状態。
pub struct Ctx<'a> {
    pub fps: Rational,
    /// 出力の大きさ。
    pub frame: [f64; 2],
    pub media: &'a HashMap<String, Media>,
    pub report: Report,
}

/// クリップの置き方（geneva の意味のまま）。
struct Geometry {
    /// 素材の画素の切り抜き `[x, y, w, h]`。
    crop: [f64; 4],
    /// 素材の大きさ。
    src: [f64; 2],
    /// 切り抜きをクリップの矩形に収める倍率（`contain` なら画面に収める倍率、`none` なら 1）。
    k: f64,
    /// クリップの矩形の上のアンカー（その矩形の画素）。
    anchor: [f64; 2],
    position: Anim,
    scale: Anim,
}

/// クリップの時間。
struct Timing {
    start: RationalTime,
    duration: RationalTime,
}

/// 全部の映像と画像のレイヤーを、下から上の順のレイヤーにする。
pub fn visual_layers(ctx: &mut Ctx<'_>, layers: &[Value]) -> Result<Vec<Layer>> {
    let mut out = Vec::new();
    for (li, layer) in layers.iter().enumerate() {
        if layer.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let name = layer
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| format!("layer{li}"), str::to_owned);
        let clips = layer
            .get("clips")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut prev_end = RationalTime::ZERO;
        for (ci, clip) in clips.iter().enumerate() {
            let timing = timing(ctx, clip, prev_end)?;
            prev_end = timing.start.checked_add(timing.duration)?;
            if let Some(l) = convert_clip(ctx, &format!("{name}-{ci}"), clip, &timing)? {
                out.push(l);
            }
        }
    }
    Ok(out)
}

fn timing(ctx: &Ctx<'_>, clip: &Value, prev_end: RationalTime) -> Result<Timing> {
    let start = match clip.get("start") {
        Some(v) => syntax::time(v, ctx.fps)?,
        None => prev_end,
    };
    let duration = clip
        .get("duration")
        .map(|v| syntax::time(v, ctx.fps))
        .transpose()?
        .ok_or_else(|| MmError::InvalidArgument("duration の無いクリップ".into()))?;
    Ok(Timing { start, duration })
}

fn convert_clip(
    ctx: &mut Ctx<'_>,
    id: &str,
    clip: &Value,
    timing: &Timing,
) -> Result<Option<Layer>> {
    note_gaps(&mut ctx.report, clip);
    let source = clip.get("source").cloned().unwrap_or(Value::Null);
    let kind = source.get("kind").and_then(Value::as_str).unwrap_or("");
    let asset = source.get("asset").and_then(Value::as_str).unwrap_or("");
    let (is_video, size) = match (kind, ctx.media.get(asset).and_then(|m| m.size)) {
        ("video", Some(size)) => (true, size),
        ("image", Some(size)) => (false, size),
        _ => {
            ctx.report.add(Gap::Source);
            return Ok(None);
        }
    };
    let geo = geometry(ctx, clip, size, is_video)?;
    let mut layer = timed_layer(id, picture_node(asset, is_video), clip, timing, ctx.fps)?;
    layer.transform = transform(&geo, timing.start)?;
    layer.crop = shape::layer_crop(geo.crop, geo.src);
    if let Some(m) = clip.get("mask").filter(|m| !m.is_null()) {
        match shape::layer_mask(geo.crop, geo.k, &format!("{id}-mask"), m)? {
            Some(mask) => layer.masks.push(mask),
            None => ctx.report.add(Gap::Mask),
        }
    }
    layer.opacity = opacity_param(ctx, clip, timing.start)?;
    Ok(Some(layer))
}

fn note_gaps(report: &mut Report, clip: &Value) {
    let has = |k: &str| clip.get(k).is_some_and(|v| !v.is_null());
    if clip
        .get("effects")
        .and_then(Value::as_array)
        .is_some_and(|e| !e.is_empty())
    {
        report.add(Gap::Effects);
    }
    if clip
        .get("blend")
        .and_then(Value::as_str)
        .is_some_and(|b| b != "normal")
    {
        report.add(Gap::Blend);
    }
    if has("transition") || has("transition_out") {
        report.add(Gap::Transition);
    }
    if has("animation") {
        report.add(Gap::Animation);
    }
    if clip
        .pointer("/transform/rotation")
        .and_then(Value::as_f64)
        .is_some_and(|r| r != 0.0)
    {
        report.add(Gap::Rotation);
    }
}

fn picture_node(asset: &str, is_video: bool) -> Node {
    let kind = if is_video { FOOTAGE_KIND } else { IMAGE_KIND };
    Node::new(kind, KIND_VERSION)
        .with_param(ASSET_PARAM, ParamValue::AssetRef(Id::new(asset)))
        .with_param(FIT_PARAM, ParamValue::Choice(FIT_NONE.into()))
}

fn geometry(ctx: &mut Ctx<'_>, clip: &Value, size: [u32; 2], is_video: bool) -> Result<Geometry> {
    let src = [f64::from(size[0]), f64::from(size[1])];
    let crop = shape::crop_rect(clip.get("crop"), src)?;
    let default_fit = if is_video { "contain" } else { FIT_NONE };
    let fit = clip
        .get("fit")
        .and_then(Value::as_str)
        .unwrap_or(default_fit);
    if fit == "cover" || fit == "fill" {
        ctx.report.add(Gap::FitCoverFill);
    }
    let k = if fit == FIT_NONE {
        1.0
    } else {
        (ctx.frame[0] / crop[2]).min(ctx.frame[1] / crop[3])
    };
    let box_size = [crop[2] * k, crop[3] * k];
    let t = clip.get("transform").cloned().unwrap_or(Value::Null);
    let anchor = match t.get("anchor") {
        Some(a) => syntax::point(a, box_size)?,
        None => [box_size[0] / 2.0, box_size[1] / 2.0],
    };
    let frame = ctx.frame;
    let position = match t.get("position") {
        Some(p) => syntax::anim(p, ctx.fps, &|v| syntax::point(v, frame))?,
        None => Anim::constant([frame[0] / 2.0, frame[1] / 2.0]),
    };
    let scale = match t.get("scale") {
        Some(s) => syntax::anim(s, ctx.fps, &syntax::scale)?,
        None => Anim::constant([1.0, 1.0]),
    };
    if position.has_easing() || scale.has_easing() {
        ctx.report.add(Gap::Easing);
    }
    Ok(Geometry {
        crop,
        src,
        k,
        anchor,
        position,
        scale,
    })
}

fn key_times(geo: &Geometry) -> Vec<RationalTime> {
    let mut times: Vec<RationalTime> = geo
        .position
        .keys
        .iter()
        .chain(&geo.scale.keys)
        .map(|k| k.time)
        .collect();
    times.sort();
    times.dedup();
    times
}

/// 素材の画素 `p` が `P + s (k (p - crop) - A)` へ移る変形（アンカーは原点）。
fn transform(geo: &Geometry, start: RationalTime) -> Result<Transform> {
    let offset = [geo.crop[0], geo.crop[1]];
    let at = |t: RationalTime| {
        let (p, s) = (geo.position.at(t), geo.scale.at(t));
        let sm = [s[0] * geo.k, s[1] * geo.k];
        let pm = [0, 1].map(|c| p[c] - sm[c] * offset[c] - s[c] * geo.anchor[c]);
        (pm, sm)
    };
    let mut out = Transform {
        anchor: Some(Param::constant(ParamValue::Vec2([0.0, 0.0]))),
        ..Transform::default()
    };
    if !geo.position.is_animated() && !geo.scale.is_animated() {
        let (pm, sm) = at(RationalTime::ZERO);
        out.position = Param::constant(ParamValue::Vec2(pm));
        out.scale = Param::constant(ParamValue::Vec2(sm));
        return Ok(out);
    }
    let (mut pos, mut scl) = (Vec::new(), Vec::new());
    for t in key_times(geo) {
        let (pm, sm) = at(t);
        let time = start.checked_add(t)?;
        pos.push(Keyframe::new(
            time,
            ParamValue::Vec2(pm),
            Interpolation::Linear,
        ));
        scl.push(Keyframe::new(
            time,
            ParamValue::Vec2(sm),
            Interpolation::Linear,
        ));
    }
    out.position = animated(pos);
    out.scale = animated(scl);
    Ok(out)
}

/// キーフレームの並びのパラメータ。`value` は最初のキーの値にする。
pub fn animated(keys: Vec<Keyframe>) -> Param {
    Param {
        value: keys[0].value.clone(),
        keyframes: keys,
    }
}

fn opacity_param(ctx: &mut Ctx<'_>, clip: &Value, start: RationalTime) -> Result<Param> {
    let Some(v) = clip.get("opacity") else {
        return Ok(Param::constant(ParamValue::Number(1.0)));
    };
    let a = syntax::anim(v, ctx.fps, &syntax::number)?;
    if a.has_easing() {
        ctx.report.add(Gap::Easing);
    }
    if !a.is_animated() {
        return Ok(Param::constant(ParamValue::Number(a.keys[0].value[0])));
    }
    let mut keys = Vec::with_capacity(a.keys.len());
    for k in &a.keys {
        let time = start.checked_add(k.time)?;
        keys.push(Keyframe::new(
            time,
            ParamValue::Number(k.value[0]),
            Interpolation::Linear,
        ));
    }
    Ok(animated(keys))
}

/// 時間だけを持つレイヤー。映像は `source.in` と `speed` から素材の範囲を決め、静止画は 0 から。
fn timed_layer(
    id: &str,
    node: Node,
    clip: &Value,
    timing: &Timing,
    fps: Rational,
) -> Result<Layer> {
    let source = clip.get("source").unwrap_or(&Value::Null);
    let speed = match clip.get("speed") {
        Some(v) => Some(syntax::decimal(&v.to_string())?),
        None => None,
    };
    let in_point = match source.get("in") {
        Some(v) => syntax::time(v, fps)?,
        None => RationalTime::ZERO,
    };
    let span = match speed {
        Some(s) => scale_time(timing.duration, s)?,
        None => timing.duration,
    };
    let mut layer = Layer::new(
        id,
        node,
        timing.start,
        in_point,
        in_point.checked_add(span)?,
    );
    if let Some(s) = speed {
        layer.speed = Rational::new(u32::try_from(s.num()).unwrap_or(1), s.den())?;
    }
    if source.get("kind").and_then(Value::as_str) == Some("video") {
        let muted = source.get("audio").and_then(Value::as_bool) == Some(false);
        layer.audio = Some(LayerAudio {
            mute: muted,
            ..LayerAudio::new("video")
        });
    }
    Ok(layer)
}

fn scale_time(t: RationalTime, by: RationalTime) -> Result<RationalTime> {
    let r = Rational::new(u32::try_from(by.num()).unwrap_or(1), by.den())?;
    t.scale(r)
}

#[cfg(test)]
mod tests;
