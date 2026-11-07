//! プロジェクトのデータのバージョンの移行。古い JSON を今のバージョンに上げてから `Project` に読む。

use serde_json::Value;

use mm_types::{MmError, RationalTime, Result};

use crate::{Project, SCHEMA_VERSION};

/// v1 の `transform.*` パラメータの前置き。
const V1_TRANSFORM_PREFIX: &str = "transform.";

/// 移行の始点（これより古いバージョンは移行が無い）。
const OLDEST_VERSION: u32 = 1;

/// JSON の文字列を読み、古いバージョンなら今のバージョンに上げ、ID で引く列を ID の順に並べ（[`Project::normalize`]）、
/// プロジェクトのデータの不変条件（[`Project::validate`]）を確かめる。
pub fn load(json: &str) -> Result<Project> {
    let value: Value = serde_json::from_str(json)?;
    let value = migrate(value)?;
    let mut project: Project = serde_json::from_value(value)?;
    project.normalize();
    project.validate()?;
    Ok(project)
}

/// JSON の値を整数のバージョンで v9 まで上げ、まだ適用していない名前つきの移行を適用する。
/// 始点より古いバージョン、今のバージョンより新しいバージョン、このビルドが知らない名前つきの移行は `Unsupported`。
pub fn migrate(mut value: Value) -> Result<Value> {
    let mut version = schema_version(&value)?;
    if version < OLDEST_VERSION {
        return Err(MmError::Unsupported(format!(
            "プロジェクトのデータのバージョン {version} は古すぎる（移行できるのは {OLDEST_VERSION} 以降）"
        )));
    }
    if version > SCHEMA_VERSION {
        return Err(MmError::Unsupported(format!(
            "プロジェクトのデータのバージョン {version} はこのビルド（{SCHEMA_VERSION}）より新しい"
        )));
    }
    while version < SCHEMA_VERSION {
        match version {
            1 => v1_to_v2(&mut value),
            2 => v2_to_v3(&mut value),
            3 => v3_to_v4(&mut value),
            4 => crate::migrate_lut::v4_to_v5(&mut value),
            5 => v5_to_v6(&mut value),
            6 => {} // v7 の delivery は serde の既定で埋まる
            7 => crate::migrate_shorts::v7_to_v8(&mut value),
            8 => {} // v9 の delivery.burn_in_captions は serde の既定（偽）で埋まる
            _ => {
                return Err(MmError::Unsupported(format!(
                    "プロジェクトのデータのバージョン {version} からの移行が無い"
                )))
            }
        }
        version += 1;
        value["schema_version"] = Value::from(version);
    }
    crate::migrate_named::apply_named(&mut value)?;
    Ok(value)
}

fn schema_version(value: &Value) -> Result<u32> {
    let raw = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| MmError::InvalidArgument("schema_version が無い".into()))?;
    u32::try_from(raw)
        .map_err(|_| MmError::InvalidArgument(format!("schema_version {raw} は u32 に収まらない")))
}

/// v1 → v2: レイヤーのノードのパラメータ `transform.*` を `Layer.transform` に移す。
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

/// v2 → v3: `sequences` と各コンポジションの `markers` を空で置く。
/// レイヤーとキーフレームに増えたフィールドは serde の既定で埋まるので、ここでは触らない。
fn v2_to_v3(value: &mut Value) {
    if value.get("sequences").is_none() {
        value["sequences"] = Value::Array(Vec::new());
    }
    let Some(comps) = value.get_mut("compositions").and_then(Value::as_array_mut) else {
        return;
    };
    for comp in comps {
        if comp.get("markers").is_none() {
            comp["markers"] = Value::Array(Vec::new());
        }
    }
}

/// v5 → v6: 素材の整理と文字起こしの修正を空で置く。
fn v5_to_v6(value: &mut Value) {
    if value.get("bins").is_none() {
        value["bins"] = serde_json::json!({ "marks": [], "selections": [], "topics": [] });
    }
    if value.get("transcript_edits").is_none() {
        value["transcript_edits"] = Value::Array(Vec::new());
    }
}

/// v3 → v4 でずらす幅。1 フレーム（最も細かい fps でも 1/240 秒程度）より十分小さい。
const MARKER_NUDGE: (i64, u32) = (1, 1000);

/// v3 → v4: 同じ時刻のマーカーの後ろの方を `MARKER_NUDGE` ずつ後ろへずらし、時刻を厳密な昇順にする。
/// 昇順でない並び（時刻が戻る）は直さず、`validate` に拒否させる。
fn v3_to_v4(value: &mut Value) {
    let Some(comps) = value.get_mut("compositions").and_then(Value::as_array_mut) else {
        return;
    };
    for comp in comps {
        let Some(markers) = comp.get_mut("markers").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut prev: Option<(RationalTime, RationalTime)> = None;
        for marker in markers {
            let Some(orig) = marker
                .get("time")
                .and_then(|t| serde_json::from_value::<RationalTime>(t.clone()).ok())
            else {
                prev = None;
                continue;
            };
            let mut time = orig;
            if let Some((prev_orig, prev_new)) = prev {
                if orig >= prev_orig && orig <= prev_new {
                    time = nudge(prev_new).unwrap_or(orig);
                }
            }
            if time != orig {
                if let Ok(t) = serde_json::to_value(time) {
                    marker["time"] = t;
                }
            }
            prev = Some((orig, time));
        }
    }
}

fn nudge(t: RationalTime) -> Option<RationalTime> {
    let step = RationalTime::new(i128::from(MARKER_NUDGE.0), MARKER_NUDGE.1).ok()?;
    t.checked_add(step).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_types::{ParamValue, Rational};

    use crate::{BlendMode, Ease, Interpolation};

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
        assert_eq!(layer.source.params.len(), 1, "ノードのパラメータだけ残る");
        assert_eq!(
            layer.transform.position,
            crate::Transform::default().position
        );
    }

    fn fixture(version: u32) -> Project {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../fixtures/doc/v{version}/stage0.json"));
        load(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn 凍結した_v1_のプロジェクトのデータが読める() {
        let p = fixture(1);
        assert_eq!(p.schema_version, SCHEMA_VERSION);
        assert_eq!(p.compositions[0].layers.len(), 2);
        assert_eq!(
            p.compositions[0].layers[0].in_point,
            RationalTime::new(1, 2).unwrap()
        );
        assert!(p.sequences.is_empty());
    }

    #[test]
    fn 凍結した_v2_のプロジェクトのデータは内容を保って_v3_に上がる() {
        let p = fixture(2);
        assert_eq!(p.schema_version, SCHEMA_VERSION);
        let text = &p.compositions[0].layers[1];
        assert_eq!(text.transform.position.keyframes.len(), 2);
        assert_eq!(
            text.transform.position.keyframes[1].value,
            ParamValue::Vec2([40.0, 0.0])
        );
        assert_eq!(text.transform.scale.value, ParamValue::Number(1.5));
        assert_eq!(
            text.opacity.keyframes[0].interpolation,
            Interpolation::Bezier
        );
        assert_eq!(text.opacity.keyframes[0].ease, Ease::None);
        assert!(text.opacity.keyframes[0].bezier.is_none());
        assert_eq!(text.speed, Rational::integer(1));
        assert!(text.enabled && text.time_remap.is_none() && text.effects.is_empty());
        assert_eq!(text.blend, BlendMode::Normal);
        assert_eq!(
            p.compositions[0].layers[0].audio.as_ref().unwrap().role,
            "voice"
        );
        assert_eq!(p.render_targets[0].codec, "h264");
    }

    #[test]
    fn 凍結した_v3_の同じ時刻のマーカーは_v4_でずれて_他は動かない() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/doc/v3/stage1.json");
        let p = load(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(p.schema_version, SCHEMA_VERSION);
        let times: Vec<_> = p.compositions[0].markers.iter().map(|m| m.time).collect();
        assert_eq!(times[0], RationalTime::seconds(1));
        assert_eq!(times[1], RationalTime::new(1001, 1000).unwrap());
        assert_eq!(times[2], RationalTime::seconds(5));
    }

    #[test]
    fn 凍結した_v4_のプロジェクトのデータは_v3_から上げたプロジェクトのデータと同じ() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let read = |v: &str| load(&std::fs::read_to_string(dir.join(v)).unwrap()).unwrap();
        assert_eq!(read("v3/stage1.json"), read("v4/stage1.json"));
    }

    #[test]
    fn 凍結した_v5_のプロジェクトのデータは_v4_から上げたプロジェクトのデータと同じ() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let read = |v: &str| load(&std::fs::read_to_string(dir.join(v)).unwrap()).unwrap();
        assert_eq!(read("v4/stage1.json"), read("v5/stage1.json"));
    }

    #[test]
    fn v5_は空の整理と直しで_v6_に上がる() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let text = std::fs::read_to_string(dir.join("v5/stage1.json")).unwrap();
        let up = migrate(serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(up["schema_version"], SCHEMA_VERSION);
        assert_eq!(up["bins"]["topics"], Value::Array(Vec::new()));
        assert_eq!(up["transcript_edits"], Value::Array(Vec::new()));
        assert!(load(&text).unwrap().bins.topics.is_empty());
    }

    #[test]
    fn 凍結した_v6_の整理と直しが読めて往復する() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let p = load(&std::fs::read_to_string(dir.join("v6/stage3.json")).unwrap()).unwrap();
        assert_eq!(p.bins.selections[0].rating, crate::Rating::Favorite);
        assert_eq!(p.bins.topics[0].title, "挨拶");
        assert_eq!(p.bins.marks[0].out_point, None);
        assert!(p.transcript_edits[0].deleted);
        let back = load(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn v6_は何も添えない納品の設定で_v7_に上がる() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let text = std::fs::read_to_string(dir.join("v6/stage3.json")).unwrap();
        let mut v6: Value = serde_json::from_str(&text).unwrap();
        v6["render_targets"] = serde_json::json!([{
            "id": "t", "composition": "c", "width": 16, "height": 9, "fps": {"num": 30, "den": 1},
            "codec": "h264", "output_path": "x.mp4", "loudness_lufs": -14.0
        }]);
        let up = migrate(v6).unwrap();
        assert_eq!(up["schema_version"], SCHEMA_VERSION);
        let t: crate::RenderTarget =
            serde_json::from_value(up["render_targets"][0].clone()).unwrap();
        assert_eq!(t.delivery, crate::Delivery::default());
        assert_eq!(t.delivery.ceiling_dbtp(), crate::DEFAULT_TRUE_PEAK_DBTP);
    }

    #[test]
    fn 凍結した_v7_の納品の設定が読めて往復する() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let p = load(&std::fs::read_to_string(dir.join("v7/stage4.json")).unwrap()).unwrap();
        let d = &p.render_targets[0].delivery;
        assert_eq!(d.subtitles, crate::SubtitleOutput::Sidecar);
        assert!(d.chapters);
        assert_eq!(d.ceiling_dbtp(), -1.5);
        assert_eq!(p.compositions[0].markers[0].name, "冒頭");
        assert_eq!(load(&serde_json::to_string(&p).unwrap()).unwrap(), p);
    }

    #[test]
    fn v8_以前の納品の設定は字幕を焼き込まないで_v9_に上がる() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let p = load(&std::fs::read_to_string(dir.join("v7/stage4.json")).unwrap()).unwrap();
        assert_eq!(p.schema_version, SCHEMA_VERSION);
        assert!(!p.render_targets[0].delivery.burn_in_captions);
        assert!(p.render_targets[0].delivery.chapters, "ほかの設定は残る");
    }

    #[test]
    fn 凍結した_v9_の字幕の焼き込みが読めて往復する() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let p = load(&std::fs::read_to_string(dir.join("v9/burnin.json")).unwrap()).unwrap();
        assert!(p.render_targets[0].delivery.burn_in_captions);
        assert_eq!(load(&serde_json::to_string(&p).unwrap()).unwrap(), p);
    }

    #[test]
    fn v1_と_v2_のフィクスチャは同じプロジェクトのデータになる() {
        let mut v1 = fixture(1);
        let v2 = fixture(2);
        // v2 のフィクスチャは v1 にキーフレームと拡大を足したものなので、その 2 つを除いて同じ。
        let text = &mut v1.compositions[0].layers[1];
        text.transform.position = v2.compositions[0].layers[1].transform.position.clone();
        text.transform.scale = v2.compositions[0].layers[1].transform.scale.clone();
        text.opacity = v2.compositions[0].layers[1].opacity.clone();
        assert_eq!(v1, v2);
    }

    #[test]
    fn 今のバージョンの_json_は往復で変わらない() {
        let p = fixture(2);
        let s = serde_json::to_string_pretty(&p).unwrap();
        let back = load(&s).unwrap();
        assert_eq!(p, back);
        assert_eq!(s, serde_json::to_string_pretty(&back).unwrap());
    }

    #[test]
    fn 新しすぎるバージョンは拒否する() {
        let v = serde_json::json!({"schema_version": SCHEMA_VERSION + 1});
        assert!(matches!(migrate(v), Err(MmError::Unsupported(_))));
    }

    #[test]
    fn バージョン_0_は_panic_せず拒否する() {
        let json =
            r#"{"schema_version": 0, "assets": [], "compositions": [], "render_targets": []}"#;
        assert!(matches!(load(json), Err(MmError::Unsupported(_))));
    }

    #[test]
    fn u32_に収まらないバージョンは_1_に丸めず拒否する() {
        let v = serde_json::json!({"schema_version": (1u64 << 32) + 1});
        assert!(matches!(migrate(v), Err(MmError::InvalidArgument(_))));
    }

    fn doc_with_duration(num: i64) -> String {
        format!(
            r#"{{"schema_version": 3, "assets": [], "render_targets": [],
              "compositions": [{{"id": "c", "width": 16, "height": 9,
                "fps": {{"num": 30, "den": 1}}, "duration": {{"num": {num}, "den": 1}},
                "layers": []}}]}}"#
        )
    }

    #[test]
    fn 長さが負のコンポジションは読み込みで拒否する() {
        assert!(load(&doc_with_duration(1)).is_ok());
        assert!(matches!(
            load(&doc_with_duration(-1)),
            Err(MmError::InvalidArgument(_))
        ));
    }

    #[test]
    fn fps_が_0_のプロジェクトのデータは読み込みで拒否する() {
        let json = doc_with_duration(1).replace(r#""num": 30, "den": 1"#, r#""num": 30, "den": 0"#);
        assert!(load(&json).is_err());
    }
}
