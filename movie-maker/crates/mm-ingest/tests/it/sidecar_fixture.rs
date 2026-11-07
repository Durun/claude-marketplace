//! 外部のツール（`tools/transcribe`）が書いた付属ファイルを、Rust 側が読めるか。
//! `fixtures/ingest/transcript-ja.json` は日本語の短いフィクスチャの音声を faster-whisper で文字起こしした出力。

use std::path::Path;

use mm_ingest_api::{parse, SidecarBody};
use mm_types::RationalTime;

#[test]
fn faster_whisper_の出力が読めて_単語の時刻が区間に収まる() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest/transcript-ja.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let sidecar = parse(value).unwrap();
    assert!(sidecar.producer.starts_with("faster-whisper"));
    let SidecarBody::Transcript(t) = sidecar.body else {
        panic!("文字起こしでない");
    };
    assert_eq!(t.language, "ja");
    let text: String = t.segments.iter().map(|s| s.text.as_str()).collect();
    assert!(text.contains("文字起こし"), "{text}");
    for seg in &t.segments {
        assert!(seg.start < seg.end);
        assert!(!seg.words.is_empty());
        for w in &seg.words {
            assert!(w.start >= RationalTime::ZERO && w.start <= w.end);
        }
    }
}
