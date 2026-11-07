//! sequence から OTIO の `Timeline` への変換。

use serde_json::{json, Value};

use mm_types::{MmError, Rational, RationalTime, Result};

use super::time::{range_to_json, time_to_json};
use super::{meta_json, sequence_layer, CutMeta, TrackMeta, META_VERSION, NAMESPACE};
use crate::{
    Asset, Composition, Cut, Layer, Node, Project, SequenceId, Transition, FOOTAGE_KIND,
    FOOTAGE_PARAM,
};

/// 素材の fps が無い（可変フレームレート）ときと、素材も無いときに使う fps。
const FALLBACK_FPS: u32 = 30;
/// ディゾルブのノードの種類の名前と、OTIO の対応する `transition_type`。
pub(super) const DISSOLVE_KIND: &str = "transition.dissolve";
pub(super) const DISSOLVE_TYPE: &str = "SMPTE_Dissolve";
/// 映像トラックの名前。
const TRACK_NAME: &str = "V1";
/// マーカーの色。OTIO の色は名前の列挙で、movie-maker のマーカーは色を持たないので既定の赤にする。
const MARKER_COLOR: &str = "RED";

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

/// OTIO のオブジェクトの共通のフィールド（metadata、name）を持つ土台。
fn object(schema: &str, name: &str, metadata: Value) -> Value {
    json!({"OTIO_SCHEMA": schema, "name": name, "metadata": metadata})
}

/// sequence `id` を OTIO の `Timeline` の JSON にする。
///
/// 単位は 1 トラック（映像）。トラックの長さは有効なカットの長さの和で、外したカットは長さ 0 の無効なクリップとして
/// 残す（OTIO は無効なクリップも長さを数えるので、長さを数えると配置がずれる）。
pub fn export_sequence(project: &Project, id: &SequenceId) -> Result<Value> {
    let seq = project.sequence(id)?;
    let host = project
        .compositions
        .iter()
        .find_map(|c| sequence_layer(c, id.as_str()).map(|l| (c, l)));
    let timeline_rate = timeline_rate(project, seq, host.map(|(c, _)| c));
    let mut children = Vec::with_capacity(seq.cuts.len());
    for (index, cut) in seq.cuts.iter().enumerate() {
        if let (Some(t), true) = (&cut.transition_in, index > 0) {
            children.push(transition_json(t, timeline_rate)?);
        }
        children.push(cut_json(project, cut, timeline_rate)?);
    }
    let markers = match host {
        Some((comp, layer)) => marker_jsons(comp, layer, timeline_rate)?,
        None => Vec::new(),
    };
    let meta = TrackMeta {
        version: META_VERSION,
        sequence: id.to_string(),
        origin: seq.origin.clone(),
        composition: host.map(|(c, _)| host_composition(c, id.as_str())),
    };
    let mut track = object("Track.1", TRACK_NAME, meta_json(&meta)?);
    track["kind"] = json!("Video");
    track["children"] = Value::Array(children);
    track["markers"] = Value::Array(markers);
    track["effects"] = json!([]);
    track["enabled"] = json!(true);
    track["source_range"] = Value::Null;
    let mut stack = object("Stack.1", "tracks", json!({}));
    stack["children"] = json!([track]);
    stack["markers"] = json!([]);
    stack["effects"] = json!([]);
    stack["enabled"] = json!(true);
    stack["source_range"] = Value::Null;
    let mut timeline = object("Timeline.1", id.as_str(), json!({}));
    timeline["global_start_time"] = Value::Null;
    timeline["tracks"] = stack;
    Ok(timeline)
}

/// メタデータに入れるコンポジション。OTIO に出すのはこの sequence のカットの素材だけなので、ほかの素材や
/// コンポジションを参照するレイヤーを入れると取り込む先で参照が切れる。この sequence を置いたレイヤーだけを残し、
/// 外したレイヤーへの親とトラックマットを外す。マーカーは OTIO のマーカーに出すので空にする。
fn host_composition(comp: &Composition, seq: &str) -> Composition {
    let keep = |l: &crate::Layer| {
        l.source.kind == crate::SEQUENCE_KIND
            && l.source.text_param(crate::SEQUENCE_PARAM) == Some(seq)
    };
    let kept: Vec<String> = comp
        .layers
        .iter()
        .filter(|l| keep(l))
        .map(|l| l.id.to_string())
        .collect();
    let layers = comp
        .layers
        .iter()
        .filter(|l| keep(l))
        .cloned()
        .map(|mut l| {
            if l.parent
                .as_ref()
                .is_some_and(|p| !kept.contains(&p.to_string()))
            {
                l.parent = None;
            }
            if l.track_matte
                .as_ref()
                .is_some_and(|m| !kept.contains(&m.layer.to_string()))
            {
                l.track_matte = None;
            }
            l
        })
        .collect();
    Composition {
        markers: Vec::new(),
        layers,
        ..comp.clone()
    }
}

/// タイムラインの fps。sequence を置いたコンポジションの fps、無ければ最初の素材の fps、それも無ければ 30。
fn timeline_rate(project: &Project, seq: &crate::Sequence, comp: Option<&Composition>) -> Rational {
    comp.map(|c| c.fps)
        .or_else(|| {
            seq.cuts
                .iter()
                .find_map(|c| footage_asset(project, &c.source).and_then(|a| a.fps))
        })
        .unwrap_or(Rational::integer(FALLBACK_FPS))
}

/// ノードが撮影素材なら、その素材。
fn footage_asset<'a>(project: &'a Project, node: &Node) -> Option<&'a Asset> {
    if node.kind != FOOTAGE_KIND {
        return None;
    }
    let id = match &node.params.get(FOOTAGE_PARAM)?.value {
        mm_types::ParamValue::AssetRef(id) => id,
        _ => return None,
    };
    project.asset(id).ok()
}

fn cut_json(project: &Project, cut: &Cut, timeline_rate: Rational) -> Result<Value> {
    let asset = footage_asset(project, &cut.source);
    if cut.source.kind == FOOTAGE_KIND && asset.is_none() {
        return Err(invalid(format!("カット {} の素材が無い", cut.id)));
    }
    let rate = asset.and_then(|a| a.fps).unwrap_or(timeline_rate);
    // OTIO の長さは配置の長さ。外したカットは配置から除かれるので 0。
    let duration = if cut.enabled {
        cut.duration()?
    } else {
        RationalTime::ZERO
    };
    let plain_footage = asset.is_some_and(|a| cut.source == Node::footage(&a.id));
    let meta = CutMeta {
        id: cut.id.to_string(),
        in_point: cut.in_point,
        out_point: cut.out_point,
        speed: cut.speed,
        enabled: cut.enabled,
        node: (!plain_footage && !cut.source.is_gap()).then(|| cut.source.clone()),
        transition_in: cut.transition_in.clone(),
        origin: cut.origin.clone(),
        overrides: cut.overrides,
        origin_base: cut.origin_base.clone(),
    };
    let metadata = meta_json(&meta)?;
    if cut.source.is_gap() {
        let mut gap = object("Gap.1", "", metadata);
        gap["source_range"] = range_to_json(RationalTime::ZERO, duration, rate)?;
        return Ok(with_item_defaults(gap, cut.enabled));
    }
    let mut clip = object("Clip.1", cut.id.as_str(), metadata);
    clip["source_range"] = range_to_json(cut.in_point, duration, rate)?;
    clip["media_reference"] = match asset {
        Some(a) => external_reference(a, timeline_rate)?,
        None => generator_reference(&cut.source),
    };
    clip["effects"] = effects_json(cut.speed);
    Ok(with_item_defaults(clip, cut.enabled))
}

/// 項目の共通のフィールドの既定値（空の効果とマーカー）を足す。クリップが先に効果を書いていたら残す。
fn with_item_defaults(mut item: Value, enabled: bool) -> Value {
    item["enabled"] = json!(enabled);
    for key in ["effects", "markers"] {
        if item.get(key).is_none() {
            item[key] = json!([]);
        }
    }
    item
}

/// 等速でなければ `LinearTimeWarp`。`time_scalar` は素材が進む速さの倍率で、2 なら倍速。
fn effects_json(speed: Rational) -> Value {
    if speed == Rational::integer(1) {
        return json!([]);
    }
    let mut effect = object("LinearTimeWarp.1", "", json!({}));
    effect["effect_name"] = json!("LinearTimeWarp");
    effect["time_scalar"] = json!(speed.to_f64());
    json!([effect])
}

fn external_reference(asset: &Asset, timeline_rate: Rational) -> Result<Value> {
    let rate = asset.fps.unwrap_or(timeline_rate);
    let mut reference = object(
        "ExternalReference.1",
        asset.id.as_str(),
        meta_json(&asset_meta(asset))?,
    );
    reference["target_url"] = json!(path_to_url(&asset.path));
    reference["available_range"] = range_to_json(RationalTime::ZERO, asset.duration, rate)?;
    Ok(reference)
}

/// 素材の厳密なコピー。メタデータの `asset` に入れる。
fn asset_meta(asset: &Asset) -> Value {
    json!({ "asset": asset })
}

/// 撮影素材でないノード（単色、入れ子など）の参照。OTIO の読み手には中身が分からない生成器として見える。
fn generator_reference(node: &Node) -> Value {
    let mut reference = object("GeneratorReference.1", "", json!({}));
    reference["generator_kind"] = json!(node.kind);
    reference["parameters"] = json!({});
    reference["available_range"] = Value::Null;
    reference
}

fn transition_json(t: &Transition, rate: Rational) -> Result<Value> {
    let half = t.duration.div_by(Rational::integer(2))?;
    let mut out = object("Transition.1", "", json!({}));
    out["transition_type"] = json!(if t.node.kind == DISSOLVE_KIND {
        DISSOLVE_TYPE
    } else {
        t.node.kind.as_str()
    });
    out["in_offset"] = time_to_json(half, rate)?;
    out["out_offset"] = time_to_json(half, rate)?;
    Ok(out)
}

/// コンポジションのマーカーを、トラックの時間軸のマーカーにする。
fn marker_jsons(comp: &Composition, layer: &Layer, rate: Rational) -> Result<Vec<Value>> {
    comp.markers
        .iter()
        .map(|m| {
            let start = m
                .time
                .checked_sub(layer.start)?
                .scale(layer.speed)?
                .checked_add(layer.in_point)?;
            let duration = m.duration.scale(layer.speed)?;
            let mut out = object("Marker.2", &m.name, json!({ NAMESPACE: {"id": m.id} }));
            out["color"] = json!(MARKER_COLOR);
            out["comment"] = json!("");
            out["marked_range"] = range_to_json(start, duration, rate)?;
            Ok(out)
        })
        .collect()
}

/// 素材の場所を OTIO の `target_url` にする。絶対パスは `file://` の URL（非 ASCII と空白は % で符号化）、
/// 相対パスはそのまま。
pub(super) fn path_to_url(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    let bytes = slashed.as_bytes();
    let absolute_windows = bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let prefix = if absolute_windows {
        "file:///"
    } else if slashed.starts_with('/') {
        "file://"
    } else {
        return slashed;
    };
    let mut out = String::from(prefix);
    for byte in slashed.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn パスは_file_url_になる() {
        assert_eq!(path_to_url("D:\\dev\\a b.mp4"), "file:///D:/dev/a%20b.mp4");
        assert_eq!(path_to_url("/mnt/x.mp4"), "file:///mnt/x.mp4");
        assert_eq!(path_to_url("media/x.mp4"), "media/x.mp4");
        assert_eq!(path_to_url("D:/日.mp4"), "file:///D:/%E6%97%A5.mp4");
    }
}
