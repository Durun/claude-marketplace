//! `schema/sidecar.schema.json` が今の型と一致するか。
//! 変えたときは `MM_UPDATE_SCHEMA=1 cargo test -p mm-ingest-api --test schema` で書き直す。

use std::path::Path;

use mm_ingest_api::{sidecar_schema_json, SCHEMA_PATH};

#[test]
fn 凍結したスキーマは最新() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH);
    let now = sidecar_schema_json();
    if std::env::var_os("MM_UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &now).unwrap();
    }
    let frozen = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        frozen, now,
        "schema/sidecar.schema.json が古い。MM_UPDATE_SCHEMA=1 cargo test -p mm-ingest-api --test schema で書き直す"
    );
}

#[test]
fn スキーマは全部の種類を持つ() {
    let text = sidecar_schema_json();
    for kind in ["transcript", "waveform", "preview_media", "scenes"] {
        assert!(text.contains(&format!("\"{kind}\"")), "{kind} が無い");
    }
}
