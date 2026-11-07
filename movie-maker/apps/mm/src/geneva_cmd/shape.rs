//! クリップの見える範囲（geneva の `crop` と `mask`）を、レイヤーの切り抜きとマスクにする。
//! レイヤーは素材を `fit: none` で置くので、レイヤーの座標は素材の画素になる。

use mm_doc::{Crop, Mask, MaskRect, Param};
use mm_types::{ParamValue, Result};
use serde_json::Value;

use super::syntax;

/// geneva の `crop` を素材の画素の矩形 `[x, y, w, h]` にする。無ければ素材の全体。
pub fn crop_rect(crop: Option<&Value>, src: [f64; 2]) -> Result<[f64; 4]> {
    let Some(c) = crop.filter(|c| !c.is_null()) else {
        return Ok([0.0, 0.0, src[0], src[1]]);
    };
    let get = |k: &str, reference: f64| c.get(k).map(|v| syntax::length(v, reference)).transpose();
    let x = get("x", src[0])?.unwrap_or(0.0).clamp(0.0, src[0]);
    let y = get("y", src[1])?.unwrap_or(0.0).clamp(0.0, src[1]);
    let w = get("width", src[0])?.unwrap_or(src[0] - x).min(src[0] - x);
    let h = get("height", src[1])?.unwrap_or(src[1] - y).min(src[1] - y);
    Ok([x, y, w.max(1.0), h.max(1.0)])
}

/// 素材の画素の切り抜き `rect` を、レイヤーの切り抜き（素材の縁から削る幅）にする。削らない辺は置かない。
pub fn layer_crop(rect: [f64; 4], src: [f64; 2]) -> Crop {
    let [x, y, w, h] = rect;
    let side = |v: f64| (v > 0.0).then(|| Param::constant(ParamValue::Number(v)));
    Crop {
        left: side(x),
        top: side(y),
        right: side(src[0] - x - w),
        bottom: side(src[1] - y - h),
    }
}

/// geneva の `mask`（クリップの矩形の画素）を、素材の画素の角丸の矩形のマスクにする。クリップの矩形は切り抜き `rect` を `k` 倍したもの。
/// 楕円と画像のマスクは `None`。
pub fn layer_mask(rect: [f64; 4], k: f64, id: &str, m: &Value) -> Result<Option<Mask>> {
    let shape = m.get("shape").and_then(Value::as_str).unwrap_or("rect");
    if shape != "rect" || m.get("asset").is_some_and(|a| !a.is_null()) {
        return Ok(None);
    }
    let box_size = [rect[2] * k, rect[3] * k];
    let get = |key: &str, reference: f64, default: f64| -> Result<f64> {
        m.get(key)
            .map_or(Ok(default), |v| syntax::length(v, reference))
    };
    let (x, y) = (get("x", box_size[0], 0.0)?, get("y", box_size[1], 0.0)?);
    let w = get("width", box_size[0], box_size[0])?;
    let h = get("height", box_size[1], box_size[1])?;
    let to_src = |v: f64| v / k;
    let shape = MaskRect::new(
        [rect[0] + to_src(x), rect[1] + to_src(y)],
        [to_src(w), to_src(h)],
        to_src(get("radius", box_size[0], 0.0)?),
    );
    let feather = to_src(get("feather", box_size[0], 0.0)?);
    let mut out = Mask::rect(id, shape);
    out.feather = Param::constant(ParamValue::Number(feather));
    out.inverted = m.get("invert").and_then(Value::as_bool).unwrap_or(false);
    Ok(Some(out))
}
