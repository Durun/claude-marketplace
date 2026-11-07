//! パラメータの指し方と値の種。レイヤーが持つ効果・マスク・音のパスも出す。

use super::build::{cut_id, layer_id, pick};
use mm_doc::{Layer, Project};
use mm_ops::ParamRef;
use mm_types::ParamValue;

/// 数のパラメータ（キーフレームを置ける）のパス。
fn numeric_paths(l: &Layer) -> Vec<String> {
    let mut paths: Vec<String> = [
        "opacity",
        "transform.scale",
        "transform.rotation",
        "crop.left",
        "crop.bottom",
    ]
    .map(String::from)
    .into();
    if l.audio.is_some() {
        paths.extend(["audio.gain_db", "audio.pan"].map(String::from));
    }
    for m in &l.masks {
        paths.push(format!("masks.{}.opacity", m.id));
        paths.push(format!("masks.{}.feather", m.id));
        if m.rect.is_some() {
            paths.push(format!("masks.{}.rect.radius", m.id));
        }
    }
    for r in &l.framing.cuts {
        paths.push(format!("reframe.{}.scale", r.cut));
        paths.push(format!("reframe.{}.rotation", r.cut));
    }
    if l.framing.caption.is_some() {
        paths.push("caption.width".into());
    }
    paths
}

/// レイヤーのパスの全部。ノードのパラメータの参照（入れ子と sequence）も含む。
fn layer_paths(l: &Layer) -> Vec<String> {
    let mut paths = numeric_paths(l);
    paths.extend(
        [
            "transform.position",
            "transform.anchor",
            "source.color",
            "source.extra",
            "source.composition",
            "source.sequence",
        ]
        .map(String::from),
    );
    for m in l.masks.iter().filter(|m| m.rect.is_some()) {
        paths.push(format!("masks.{}.rect.position", m.id));
        paths.push(format!("masks.{}.rect.size", m.id));
    }
    for e in &l.effects {
        paths.push(format!("effects.{}.amount", e.id));
        paths.push(format!("effects.{}.composition", e.id));
    }
    paths
}

const CUT_PATHS: [&str; 4] = [
    "source.color",
    "source.extra",
    "source.composition",
    "source.sequence",
];

/// パラメータの指し方をプロジェクトのデータから選ぶ。`numeric` なら数のパラメータだけ。
pub fn param_ref(p: &Project, i: u8, path: u8, numeric: bool) -> Option<ParamRef> {
    if i % 2 == 1 && !numeric {
        let cut = cut_id(p, i)?;
        return Some(ParamRef::cut(&cut, *pick(&CUT_PATHS, path)?));
    }
    let id = layer_id(p, i)?;
    let (_, layer) = p.layer(&id).ok()?;
    let paths = if numeric {
        numeric_paths(layer)
    } else {
        layer_paths(layer)
    };
    Some(ParamRef::layer(&id, pick(&paths, path)?.clone()))
}

/// パスに合う値。入れ子と sequence の参照はプロジェクトのデータの ID を指す。
pub fn value_for(p: &Project, path: &str, v: i64) -> ParamValue {
    let pick_u8 = v.unsigned_abs() as u8;
    if path.ends_with(".composition") {
        let id = pick(&p.compositions, pick_u8).map_or("none", |c| c.id.as_str());
        ParamValue::Text(id.to_owned())
    } else if path.ends_with(".sequence") {
        let id = pick(&p.sequences, pick_u8).map_or("none", |s| s.id.as_str());
        ParamValue::Text(id.to_owned())
    } else if path.contains("position") || path.contains("anchor") {
        ParamValue::Vec2([v as f64, 0.0])
    } else if path.ends_with(".size") {
        ParamValue::Vec2([v.unsigned_abs() as f64, 1.0])
    } else {
        ParamValue::Number(v as f64)
    }
}
