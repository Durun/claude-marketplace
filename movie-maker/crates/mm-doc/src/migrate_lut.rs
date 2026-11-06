//! v4 → v5: `color.lut` の LUT をファイルのパス（`path`）から素材（`lut` の `AssetRef`）に移す。
//!
//! LUT の中身を書き換えるとフレームのキャッシュのキーが変わるよう、LUT を素材の `contentHash` で扱う。
//! 移行では中身を読まないので、作る素材の `contentHash` は空にする（キャッシュは空のハッシュの素材に頼る画像を
//! 覚えない）。取り込み直すとハッシュが入る。同じパスは 1 つの素材にまとめる。

use serde_json::{json, Map, Value};

use mm_types::{ColorSpace, RationalTime};

/// LUT の種類の名前と、そのパラメータの名前。
const LUT_KIND: &str = "color.lut";
const OLD_PARAM: &str = "path";
const NEW_PARAM: &str = "lut";
/// 作る素材の ID の接頭辞。後ろはパスの FNV-1a（64 bit）の 16 進。
const ASSET_PREFIX: &str = "lut-";

/// v4 → v5。プロジェクトのデータの中の全部のノード（レイヤー、効果、カット、トランジション、入力）を見る。
pub(crate) fn v4_to_v5(value: &mut Value) {
    let mut paths = Vec::new();
    for key in ["compositions", "sequences"] {
        if let Some(v) = value.get_mut(key) {
            move_lut_paths(v, &mut paths);
        }
    }
    let Some(assets) = value.get_mut("assets").and_then(Value::as_array_mut) else {
        return;
    };
    for (id, path) in paths {
        let exists = assets
            .iter()
            .any(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str()));
        if !exists {
            assets.push(lut_asset(&id, &path));
        }
    }
}

/// `value` の中の `color.lut` のノードの `path` を `lut` に移し、作る素材の ID とパスを `paths` に積む。
fn move_lut_paths(value: &mut Value, paths: &mut Vec<(String, String)>) {
    match value {
        Value::Array(items) => {
            for item in items {
                move_lut_paths(item, paths);
            }
        }
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some(LUT_KIND) {
                if let Some(params) = map.get_mut("params").and_then(Value::as_object_mut) {
                    move_one(params, paths);
                }
            }
            for item in map.values_mut() {
                move_lut_paths(item, paths);
            }
        }
        _ => {}
    }
}

fn move_one(params: &mut Map<String, Value>, paths: &mut Vec<(String, String)>) {
    let Some(old) = params.remove(OLD_PARAM) else {
        return;
    };
    let path = old
        .pointer("/value/value")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if path.is_empty() {
        return;
    }
    let id = format!("{ASSET_PREFIX}{:016x}", fnv1a(path.as_bytes()));
    params.insert(
        NEW_PARAM.to_owned(),
        json!({"value": {"type": "asset_ref", "value": id}}),
    );
    paths.push((id, path));
}

fn lut_asset(id: &str, path: &str) -> Value {
    json!({
        "id": id,
        "path": path,
        "content_hash": "",
        "color_space": ColorSpace::working(),
        "duration": RationalTime::ZERO,
        "fps": null,
    })
}

/// FNV-1a（64 bit）。素材の ID をパスから決定的に作る。
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use mm_types::{AssetId, ParamValue};

    use crate::load;

    fn v4_with_lut(path: &str) -> String {
        format!(
            r#"{{"schema_version": 4, "assets": [], "render_targets": [], "sequences": [],
              "compositions": [{{"id": "c", "width": 16, "height": 9,
                "fps": {{"num": 30, "den": 1}}, "duration": {{"num": 1, "den": 1}}, "markers": [],
                "layers": [{{
                  "id": "l", "start": {{"num": 0, "den": 1}}, "in": {{"num": 0, "den": 1}},
                  "out": {{"num": 1, "den": 1}},
                  "opacity": {{"value": {{"type": "number", "value": 1.0}}}},
                  "source": {{"kind": "solid", "version": 1, "params": {{}}}},
                  "effects": [
                    {{"id": "e1", "enabled": true, "node": {{"kind": "color.lut", "version": 1,
                      "params": {{"path": {{"value": {{"type": "text", "value": "{path}"}}}}}}}}}},
                    {{"id": "e2", "enabled": true, "node": {{"kind": "color.lut", "version": 1,
                      "params": {{"path": {{"value": {{"type": "text", "value": "{path}"}}}}}}}}}}
                  ]
                }}]}}]}}"#
        )
    }

    #[test]
    fn lut_のパスは素材になり_同じパスは_1_つにまとまる() {
        let p = load(&v4_with_lut("luts/look.cube")).unwrap();
        assert_eq!(p.assets.len(), 1, "同じパスは 1 つの素材");
        let asset = &p.assets[0];
        assert_eq!(asset.path, "luts/look.cube");
        assert!(
            asset.content_hash.is_empty(),
            "中身は読まないのでハッシュは空"
        );
        for effect in &p.compositions[0].layers[0].effects {
            let params = &effect.node.params;
            assert!(!params.contains_key("path"));
            assert_eq!(
                params["lut"].value,
                ParamValue::AssetRef(AssetId::new(asset.id.as_str()))
            );
        }
    }

    #[test]
    fn 空のパスは消すだけ() {
        let p = load(&v4_with_lut("")).unwrap();
        assert!(p.assets.is_empty());
        let params = &p.compositions[0].layers[0].effects[0].node.params;
        assert!(params.is_empty());
    }
}
