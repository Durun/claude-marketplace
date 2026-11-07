//! `schema/ops.json` が今の操作の型と一致するか。
//! 変えたときは `MM_UPDATE_SCHEMA=1 cargo test -p mm-ops --test it` で書き直す。

use std::path::Path;

use mm_ops::{ops_schema_json, Op, SCHEMA_PATH};

fn schema_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH)
}

#[test]
fn 凍結したスキーマは最新() {
    let now = ops_schema_json();
    if std::env::var_os("MM_UPDATE_SCHEMA").is_some() {
        std::fs::write(schema_path(), &now).unwrap();
    }
    let frozen = std::fs::read_to_string(schema_path()).unwrap_or_default();
    assert_eq!(
        frozen, now,
        "schema/ops.json が古い。MM_UPDATE_SCHEMA=1 cargo test -p mm-ops --test it で書き直す"
    );
}

#[test]
fn スキーマは全部の操作の名前を持つ() {
    let schema: serde_json::Value = serde_json::from_str(&ops_schema_json()).unwrap();
    let text = schema.to_string();
    for name in Op::NAMES {
        assert!(
            text.contains(&format!("\"{name}\"")),
            "{name} がスキーマに無い"
        );
    }
}
