//! 付属ファイルの契約テスト。実装は無く、スキーマの読みと版の移行を確かめる。

use mm_ingest_api::{
    migrate, parse, Sidecar, SidecarBody, Transcript, TranscriptSegment, SIDECAR_SCHEMA_VERSION,
};
use mm_types::{MmError, RationalTime};

/// 今の版の見本の付属ファイル。
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
            }],
        }),
    }
}

/// 付属ファイルの契約。`old_versions` は古い版の JSON（今の版まで上がるもの）。
pub fn contract(old_versions: &[serde_json::Value]) {
    let s = sample_sidecar();
    let v = serde_json::to_value(&s).expect("JSON にできる");
    assert_eq!(parse(v.clone()).expect("読める"), s, "今の版は往復する");
    assert_eq!(v["kind"], "transcript", "kind のタグで中身を見分ける");

    for old in old_versions {
        let migrated = migrate(old.clone()).expect("移行できる");
        assert_eq!(
            migrated["schema_version"], SIDECAR_SCHEMA_VERSION,
            "今の版まで上がる"
        );
        parse(migrated).expect("移行後は読める");
    }

    let future = serde_json::json!({ "schema_version": SIDECAR_SCHEMA_VERSION + 1 });
    assert!(
        matches!(migrate(future), Err(MmError::Unsupported(_))),
        "未来の版は断る"
    );
    let missing = serde_json::json!({ "content_hash": "x" });
    assert!(
        matches!(migrate(missing), Err(MmError::InvalidArgument(_))),
        "版の無い JSON は断る"
    );
}
