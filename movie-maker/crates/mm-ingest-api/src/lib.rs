//! 取り込みの付属ファイル（sidecar）の形。文字起こし、波形、プレビュー動画の記録。
//! 言語を問わない外の道具（`tools/`）がこの JSON を書き、`mm-ingest` が読む。
//!
//! 契約（`mm_testkit::ingest::contract`）:
//! - 今の版の JSON は `parse` を通り、serde の往復で一致する。
//! - 古い版の JSON は `migrate` で今の版に上がる。移行は 1 版ずつの純関数。
//! - 未来の版は `Err(Unsupported)`。
//! - `schema_version` の無い JSON は `Err(InvalidArgument)`。

use serde::{Deserialize, Serialize};

use mm_types::{MmError, RationalTime, Result};

/// 付属ファイルのスキーマの版。
pub const SIDECAR_SCHEMA_VERSION: u32 = 1;

/// 付属ファイル。素材の中身のハッシュで結びつける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub schema_version: u32,
    pub content_hash: String,
    /// 作った道具の名前と版（`faster-whisper 1.1.0`）。
    pub producer: String,
    #[serde(flatten)]
    pub body: SidecarBody,
}

/// 付属ファイルの中身。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SidecarBody {
    Transcript(Transcript),
    Waveform(Waveform),
    PreviewMedia(PreviewMedia),
}

/// 文字起こし。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub language: String,
    pub segments: Vec<TranscriptSegment>,
}

/// 文字起こしの 1 区間。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start: RationalTime,
    pub end: RationalTime,
    pub text: String,
}

/// 波形。ピークは -1..=1。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waveform {
    pub peaks_per_second: u32,
    pub channels: u16,
    /// チャンネルごとにインターリーブしたピーク。
    pub peaks: Vec<f32>,
}

/// プレビュー動画の記録。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewMedia {
    /// `cache/` からの相対パス。
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// 今の版の JSON を読む。版が違えば `migrate` を通す。
pub fn parse(value: serde_json::Value) -> Result<Sidecar> {
    let migrated = migrate(value)?;
    Ok(serde_json::from_value(migrated)?)
}

/// 古い版の JSON を 1 版ずつ今の版に上げる。版が無いか未来なら `Err`。
pub fn migrate(mut value: serde_json::Value) -> Result<serde_json::Value> {
    let mut version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| MmError::InvalidArgument("sidecar: schema_version が無い".into()))?
        as u32;
    if version > SIDECAR_SCHEMA_VERSION {
        return Err(MmError::Unsupported(format!(
            "sidecar: 版 {version} は新しすぎる（今は {SIDECAR_SCHEMA_VERSION}）"
        )));
    }
    while version < SIDECAR_SCHEMA_VERSION {
        value = migrate_one(value, version)?;
        version += 1;
    }
    Ok(value)
}

/// `from` 版から `from + 1` 版へ。版を上げるたびにここに 1 つ足す。
fn migrate_one(_value: serde_json::Value, from: u32) -> Result<serde_json::Value> {
    Err(MmError::Unsupported(format!(
        "sidecar: 版 {from} からの移行が無い"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 今の版は往復する() {
        let s = Sidecar {
            schema_version: SIDECAR_SCHEMA_VERSION,
            content_hash: "abc".into(),
            producer: "test".into(),
            body: SidecarBody::Waveform(Waveform {
                peaks_per_second: 10,
                channels: 1,
                peaks: vec![0.0, 0.5],
            }),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["kind"], "waveform");
        assert_eq!(parse(v).unwrap(), s);
    }

    #[test]
    fn 未来の版は断る() {
        let v = serde_json::json!({"schema_version": SIDECAR_SCHEMA_VERSION + 1});
        assert!(matches!(migrate(v), Err(MmError::Unsupported(_))));
    }
}
