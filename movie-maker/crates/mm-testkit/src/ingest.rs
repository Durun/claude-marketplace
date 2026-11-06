//! 付属ファイルの契約テスト。実装は無く、スキーマの読みとバージョンの移行を確かめる。

use mm_ingest_api::{
    migrate, parse, Sidecar, SidecarBody, Transcript, TranscriptSegment, SIDECAR_SCHEMA_VERSION,
};
use mm_types::{MmError, RationalTime};

/// 今のバージョンのテスト用の付属ファイル。
pub fn sample_sidecar() -> Sidecar {
    Sidecar {
        schema_version: SIDECAR_SCHEMA_VERSION,
        content_hash: "0123456789abcdef".into(),
        producer: "mm-testkit".into(),
        body: SidecarBody::Transcript(Transcript {
            language: "ja".into(),
            segments: vec![TranscriptSegment {
                start: RationalTime::ZERO,
                end: RationalTime::seconds(2),
                text: "こんにちは".into(),
                words: Vec::new(),
            }],
        }),
    }
}

/// 付属ファイルの契約。`old_versions` は古いバージョンの JSON（今のバージョンまで上がるもの）。
pub fn contract(old_versions: &[serde_json::Value]) {
    let s = sample_sidecar();
    let v = serde_json::to_value(&s).expect("JSON にできる");
    assert_eq!(
        parse(v.clone()).expect("読める"),
        s,
        "今のバージョンは往復する"
    );
    assert_eq!(v["kind"], "transcript", "kind のタグで内容を見分ける");

    for old in old_versions {
        let migrated = migrate(old.clone()).expect("移行できる");
        assert_eq!(
            migrated["schema_version"], SIDECAR_SCHEMA_VERSION,
            "今のバージョンまで上がる"
        );
        parse(migrated).expect("移行後は読める");
    }

    let future = serde_json::json!({ "schema_version": SIDECAR_SCHEMA_VERSION + 1 });
    assert!(
        matches!(migrate(future), Err(MmError::Unsupported(_))),
        "未来のバージョンは拒否する"
    );
    let missing = serde_json::json!({ "content_hash": "x" });
    assert!(
        matches!(migrate(missing), Err(MmError::InvalidArgument(_))),
        "バージョンの無い JSON は拒否する"
    );
}
