//! 名前つきの移行。整数のバージョン（v1〜v9）の後の形の変更は、`src/migrations/YYYYMMDD_<題>.rs` の 1 ファイルに
//! `pub(crate) fn up(value: &mut serde_json::Value)` を書いて足す。表は `build.rs` がディレクトリから作る。
//!
//! プロジェクトのデータは済ませた移行の名前の組（`migrations`）を持ち、読むときは組に無い移行だけを名前の順に適用する
//! （Rails の `schema_migrations` と sqlx の `_sqlx_migrations` と同じ型）。並行の PR が連番を取り合わず、マージの順にも依らない。
//! 移行は書いた時点の main にある移行だけを前提にし、並行に足された移行とは適用する順に依らない形にする。

use serde_json::Value;

use mm_types::{MmError, Result};

use crate::{Project, SCHEMA_VERSION};

/// プロジェクトのデータの JSON で、済ませた名前つきの移行の組を持つフィールド。
pub const MIGRATIONS_FIELD: &str = "migrations";
const VERSION_FIELD: &str = "schema_version";

/// 名前つきの移行の 1 つ。
pub(crate) struct Migration {
    /// ファイルの名前（`YYYYMMDD_<題>`）。プロジェクトのデータに書く。
    pub(crate) name: &'static str,
    /// JSON を書き換える。
    pub(crate) up: fn(&mut Value),
}

include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

/// このビルドが知る名前つきの移行の名前。名前の順。
pub fn migration_names() -> Vec<String> {
    names_of(MIGRATIONS)
}

fn names_of(registry: &[Migration]) -> Vec<String> {
    registry.iter().map(|m| m.name.to_owned()).collect()
}

/// 整数のバージョンを上げ終えた JSON に、まだ適用していない名前つきの移行を適用し、`migrations` を全部の名前にする。
pub(crate) fn apply_named(value: &mut Value) -> Result<()> {
    apply_with(value, MIGRATIONS)
}

fn apply_with(value: &mut Value, registry: &[Migration]) -> Result<()> {
    let applied = applied_names(value)?;
    reject_unknown(&applied, registry)?;
    for m in registry
        .iter()
        .filter(|m| !applied.iter().any(|a| a == m.name))
    {
        (m.up)(value);
    }
    if !registry.is_empty() {
        value[MIGRATIONS_FIELD] = Value::from(names_of(registry));
    }
    Ok(())
}

/// JSON の `migrations`。無ければ空。
fn applied_names(value: &Value) -> Result<Vec<String>> {
    let Some(raw) = value.get(MIGRATIONS_FIELD) else {
        return Ok(Vec::new());
    };
    raw.as_array()
        .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_owned)).collect())
        .ok_or_else(|| MmError::InvalidArgument(format!("{MIGRATIONS_FIELD} は文字列の列にする")))
}

/// このビルドが知らない移行を済ませたプロジェクトのデータは、新しいビルドが書いたものなので読まない。
fn reject_unknown(applied: &[String], registry: &[Migration]) -> Result<()> {
    match applied
        .iter()
        .find(|a| !registry.iter().any(|m| m.name == a.as_str()))
    {
        Some(name) => Err(MmError::Unsupported(format!(
            "プロジェクトのデータの移行 {name} はこのビルドより新しい"
        ))),
        None => Ok(()),
    }
}

/// 保存の先頭に書く、どこまで移行したかの記録（`schema_version` と、空でなければ `migrations`）。
pub fn version_stamp(p: &Project) -> Value {
    let mut stamp = serde_json::json!({ VERSION_FIELD: p.schema_version });
    if !p.migrations.is_empty() {
        stamp[MIGRATIONS_FIELD] = Value::from(p.migrations.clone());
    }
    stamp
}

/// 移行の記録（[`version_stamp`]）のフィールドを、組み立て中のプロジェクトのデータの JSON に写す。
pub fn copy_stamp(stamp: &Value, value: &mut Value) {
    for field in [VERSION_FIELD, MIGRATIONS_FIELD] {
        if let Some(v) = stamp.get(field) {
            value[field] = v.clone();
        }
    }
}

/// 移行の記録が、このビルドの移行（整数のバージョンと名前つきの全部）を済ませているか。
pub fn is_current(stamp: &Value) -> Result<bool> {
    let version = stamp.get(VERSION_FIELD).and_then(Value::as_u64);
    let applied = applied_names(stamp)?;
    Ok(version == Some(u64::from(SCHEMA_VERSION))
        && MIGRATIONS
            .iter()
            .all(|m| applied.iter().any(|a| a == m.name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_a(v: &mut Value) {
        v["a"] = Value::from(true);
    }

    fn add_b(v: &mut Value) {
        let has_a = v.get("a").is_some();
        v["b"] = Value::from(has_a);
    }

    const REGISTRY: &[Migration] = &[
        Migration {
            name: "20261101_a",
            up: add_a,
        },
        Migration {
            name: "20261102_b",
            up: add_b,
        },
    ];

    #[test]
    fn 組の無いプロジェクトのデータには全部を名前の順に適用する() {
        let mut v = serde_json::json!({});
        apply_with(&mut v, REGISTRY).unwrap();
        assert_eq!(v["a"], true);
        assert_eq!(v["b"], true, "a の後に b を適用する");
        assert_eq!(
            v[MIGRATIONS_FIELD],
            serde_json::json!(["20261101_a", "20261102_b"])
        );
    }

    #[test]
    fn 済ませた移行は適用せず_後からマージされた古い名前の移行も適用する() {
        let mut v = serde_json::json!({ MIGRATIONS_FIELD: ["20261102_b"], "b": "keep" });
        apply_with(&mut v, REGISTRY).unwrap();
        assert_eq!(v["a"], true);
        assert_eq!(v["b"], "keep");
        assert_eq!(v[MIGRATIONS_FIELD].as_array().unwrap().len(), 2);
    }

    #[test]
    fn 知らない移行を済ませたプロジェクトのデータは拒否する() {
        let mut v = serde_json::json!({ MIGRATIONS_FIELD: ["20991231_future"] });
        assert!(matches!(
            apply_with(&mut v, REGISTRY),
            Err(MmError::Unsupported(_))
        ));
    }

    #[test]
    fn 文字列の列でない組は拒否する() {
        let mut v = serde_json::json!({ MIGRATIONS_FIELD: [1] });
        assert!(matches!(
            apply_with(&mut v, REGISTRY),
            Err(MmError::InvalidArgument(_))
        ));
    }

    #[test]
    fn 表が空なら組を書き足さない() {
        let mut v = serde_json::json!({ "x": 1 });
        apply_with(&mut v, &[]).unwrap();
        assert_eq!(v, serde_json::json!({ "x": 1 }));
    }

    #[test]
    fn 表は名前の順に並ぶ() {
        let names = migration_names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn 新しいプロジェクトの移行の記録は今のビルドで済んでいる() {
        let p = Project::new();
        let stamp = version_stamp(&p);
        assert!(is_current(&stamp).unwrap());
        let mut value = serde_json::json!({});
        copy_stamp(&stamp, &mut value);
        assert_eq!(value[VERSION_FIELD], SCHEMA_VERSION);
    }

    #[test]
    fn 整数のバージョンが古い移行の記録は済んでいない() {
        assert!(!is_current(&serde_json::json!({ VERSION_FIELD: 8 })).unwrap());
    }

    #[test]
    fn 整数のバージョンは_9_で止める() {
        assert_eq!(
            SCHEMA_VERSION, 9,
            "形の変更は src/migrations/ に名前つきの移行を足す（docs/design/14-dev-structure.md の「並行開発の境界」）"
        );
    }

    /// `fixtures/doc/*/*.json` を整数のバージョンまで上げた JSON（名前つきの移行の組は外す）。
    fn fixture_values() -> Vec<(String, Value)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        let mut out = Vec::new();
        for dir in std::fs::read_dir(&root).unwrap() {
            for file in std::fs::read_dir(dir.unwrap().path()).unwrap() {
                let path = file.unwrap().path();
                if path.extension().is_some_and(|x| x == "json") {
                    let raw: Value =
                        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    let mut v = crate::migrate(raw).unwrap();
                    v.as_object_mut().unwrap().remove(MIGRATIONS_FIELD);
                    out.push((path.display().to_string(), v));
                }
            }
        }
        out
    }

    /// 並行の PR が足した移行は、データによって適用される順が変わる（既存のデータは後から、新規のデータは名前の順）。
    /// どの 2 つも順を入れ替えて同じ JSON になることを、凍結したプロジェクトのデータで確かめる。
    #[test]
    fn 名前つきの移行はどの_2_つも適用する順に依らない() {
        let values = fixture_values();
        assert!(!values.is_empty());
        for (i, a) in MIGRATIONS.iter().enumerate() {
            for b in &MIGRATIONS[i + 1..] {
                for (path, v) in &values {
                    let (mut ab, mut ba) = (v.clone(), v.clone());
                    (a.up)(&mut ab);
                    (b.up)(&mut ab);
                    (b.up)(&mut ba);
                    (a.up)(&mut ba);
                    assert_eq!(ab, ba, "{} と {} が {path} で順に依る", a.name, b.name);
                }
            }
        }
    }
}
