//! パラメータの指し方と値の種。レイヤーが持つ効果・マスク・音の道筋も出す。

use super::build::{cut_id, layer_id, pick};
use mm_doc::{Layer, Project};
use mm_ops::ParamRef;
use mm_types::ParamValue;

/// 数のパラメータ（キーフレームを置ける）の道筋。
fn numeric_paths(l: &Layer) -> Vec<String> {
    let mut paths: Vec<String> = ["opacity", "transform.scale", "transform.rotation"]
        .map(String::from)
        .into();
    if l.audio.is_some() {
        paths.extend(["audio.gain_db", "audio.pan"].map(String::from));
    }
    for m in &l.masks {
        paths.push(format!("masks.{}.opacity", m.id));
        paths.push(format!("masks.{}.feather", m.id));
    }
    paths
}

/// レイヤーの道筋の全部。種類のパラメータの参照（入れ子と sequence）も含む。
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

/// パラメータの指し方を文書から選ぶ。`numeric` なら数のパラメータだけ。
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

/// 道筋に合う値。入れ子と sequence の参照は文書の ID を指す。
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
    } else {
        ParamValue::Number(v as f64)
    }
}
