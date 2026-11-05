//! 文書の版の移行。古い JSON を今の版に上げてから `Project` に読む。

use serde_json::Value;

use mm_types::{MmError, Result};

use crate::{Project, SCHEMA_VERSION};

/// v1 の `transform.*` パラメータの前置き。
const V1_TRANSFORM_PREFIX: &str = "transform.";

/// JSON の文字列を読み、古い版なら今の版に上げる。
pub fn load(json: &str) -> Result<Project> {
    let value: Value = serde_json::from_str(json)?;
    let value = migrate(value)?;
    Ok(serde_json::from_value(value)?)
}

/// JSON の値を今の版に上げる。今の版より新しければ `Unsupported`。
pub fn migrate(mut value: Value) -> Result<Value> {
    let mut version = schema_version(&value)?;
    while version < SCHEMA_VERSION {
        match version {
            1 => v1_to_v2(&mut value),
            _ => unreachable!("版 {version} の移行は上で尽きる"),
        }
        version += 1;
        value["schema_version"] = Value::from(version);
    }
    if version > SCHEMA_VERSION {
        return Err(MmError::Unsupported(format!(
            "文書の版 {version} はこのビルド（{SCHEMA_VERSION}）より新しい"
        )));
    }
    Ok(value)
}

fn schema_version(value: &Value) -> Result<u32> {
    value
        .get("schema_version")
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .ok_or_else(|| MmError::InvalidArgument("schema_version が無い".into()))
}

/// v1 → v2: レイヤーの種類のパラメータ `transform.*` を `Layer.transform` に移す。
fn v1_to_v2(value: &mut Value) {
    let Some(comps) = value.get_mut("compositions").and_then(Value::as_array_mut) else {
        return;
    };
    for layer in comps
        .iter_mut()
        .filter_map(|c| c.get_mut("layers")?.as_array_mut())
        .flatten()
    {
        move_transform_params(layer);
    }
}

fn move_transform_params(layer: &mut Value) {
    let Some(params) = layer
        .get_mut("source")
        .and_then(|s| s.get_mut("params"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let names: Vec<String> = params
        .keys()
        .filter(|k| k.starts_with(V1_TRANSFORM_PREFIX))
        .cloned()
        .collect();
    if names.is_empty() {
        return;
    }
    let mut transform = serde_json::Map::new();
    for name in names {
        if let Some(param) = params.remove(&name) {
            transform.insert(name[V1_TRANSFORM_PREFIX.len()..].to_owned(), param);
        }
    }
    layer["transform"] = Value::Object(transform);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_types::ParamValue;

    #[test]
    fn v1_の_transform_パラメータはレイヤーに移る() {
        let json = r#"{
          "schema_version": 1, "assets": [], "render_targets": [],
          "compositions": [{
            "id": "c", "width": 16, "height": 9, "fps": {"num": 30, "den": 1},
            "duration": {"num": 1, "den": 1},
            "layers": [{
              "id": "l", "start": {"num": 0, "den": 1}, "in": {"num": 0, "den": 1},
              "out": {"num": 1, "den": 1},
              "opacity": {"value": {"type": "number", "value": 1.0}},
              "source": {"kind": "solid", "version": 1, "params": {
                "transform.scale": {"value": {"type": "number", "value": 2.0}},
                "color": {"value": {"type": "number", "value": 1.0}}
              }}
            }]
          }]
        }"#;
        let p = load(json).unwrap();
        assert_eq!(p.schema_version, SCHEMA_VERSION);
        let layer = &p.compositions[0].layers[0];
        assert_eq!(layer.transform.scale.value, ParamValue::Number(2.0));
        assert_eq!(layer.source.params.len(), 1, "種類のパラメータだけ残る");
        assert_eq!(
            layer.transform.position,
            crate::Transform::default().position
        );
    }

    #[test]
    fn 凍結した_v1_の文書が読める() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/doc/v1/stage0.json");
        let text = std::fs::read_to_string(path).unwrap();
        let p = load(&text).unwrap();
        assert_eq!(p.schema_version, SCHEMA_VERSION);
        assert_eq!(p.compositions[0].layers.len(), 2);
    }

    #[test]
    fn 新しすぎる版は断る() {
        let v = serde_json::json!({"schema_version": SCHEMA_VERSION + 1});
        assert!(matches!(migrate(v), Err(MmError::Unsupported(_))));
    }
}
