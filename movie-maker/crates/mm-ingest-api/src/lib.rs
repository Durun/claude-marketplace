//! 取り込みの付属ファイル（sidecar）の形。文字起こし、波形、プレビュー動画の記録。
//! 言語を問わない外のツール（`tools/`）がこの JSON を書き、`mm-ingest` が読む。
//!
//! 契約（`mm_testkit::ingest::contract`）:
//! - 今のバージョンの JSON は `parse` を通り、serde の往復で一致する。
//! - 古いバージョンの JSON は `migrate` で今のバージョンに上がる。移行は 1 バージョンずつの純関数。
//! - 未来のバージョンは `Err(Unsupported)`。
//! - `schema_version` の無い JSON は `Err(InvalidArgument)`。

use serde::{Deserialize, Serialize};

use mm_types::{MmError, RationalTime, Result};

/// 付属ファイルのスキーマのバージョン。
pub const SIDECAR_SCHEMA_VERSION: u32 = 1;

/// 付属ファイル。素材のコンテンツハッシュで結びつける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub schema_version: u32,
    pub content_hash: String,
    /// 作ったツールの名前とバージョン（`faster-whisper 1.1.0`）。
    pub producer: String,
    #[serde(flatten)]
    pub body: SidecarBody,
}

/// 付属ファイルの内容。
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

/// 今のバージョンの JSON を読む。バージョンが違えば `migrate` を通す。
pub fn parse(value: serde_json::Value) -> Result<Sidecar> {
    let migrated = migrate(value)?;
    Ok(serde_json::from_value(migrated)?)
}

/// 古いバージョンの JSON を 1 バージョンずつ今のバージョンに上げる。バージョンが無いか未来なら `Err`。
pub fn migrate(mut value: serde_json::Value) -> Result<serde_json::Value> {
    let mut version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| MmError::InvalidArgument("sidecar: schema_version が無い".into()))?
        as u32;
    if version > SIDECAR_SCHEMA_VERSION {
        return Err(MmError::Unsupported(format!(
            "sidecar: バージョン {version} は新しすぎる（今は {SIDECAR_SCHEMA_VERSION}）"
        )));
    }
    while version < SIDECAR_SCHEMA_VERSION {
        value = migrate_one(value, version)?;
        version += 1;
    }
    Ok(value)
}

/// バージョン `from` から `from + 1` へ。バージョンを上げるたびにここに 1 つ足す。
fn migrate_one(_value: serde_json::Value, from: u32) -> Result<serde_json::Value> {
    Err(MmError::Unsupported(format!(
        "sidecar: バージョン {from} からの移行が無い"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 今のバージョンは往復する() {
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
    fn 未来のバージョンは拒否する() {
        let v = serde_json::json!({"schema_version": SIDECAR_SCHEMA_VERSION + 1});
        assert!(matches!(migrate(v), Err(MmError::Unsupported(_))));
    }
}
