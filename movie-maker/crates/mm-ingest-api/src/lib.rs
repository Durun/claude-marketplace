//! 取り込みの付属ファイル（sidecar）の形。文字起こし、波形、プレビュー動画の記録。
//! 言語を問わない外のツール（`tools/`）がこの JSON を書き、`mm-ingest` が読む。
//!
//! 契約（`mm_testkit::ingest::contract`）:
//! - 今のバージョンの JSON は `parse` を通り、serde の往復で一致する。
//! - 古いバージョンの JSON は `migrate` で今のバージョンに上がる。移行は 1 バージョンずつの純関数。
//! - 未来のバージョンは `Err(Unsupported)`。
//! - `schema_version` の無い JSON は `Err(InvalidArgument)`。
//!
//! 外のツールが書くときの JSON Schema は [`sidecar_schema_json`]（`schema/sidecar.schema.json` に凍結）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use mm_types::{MmError, RationalTime, Result};

/// 付属ファイルのスキーマのバージョン。
pub const SIDECAR_SCHEMA_VERSION: u32 = 1;

/// 付属ファイル。素材のコンテンツハッシュで結びつける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sidecar {
    pub schema_version: u32,
    pub content_hash: String,
    /// 作ったツールの名前とバージョン（`faster-whisper 1.1.0`）。
    pub producer: String,
    #[serde(flatten)]
    pub body: SidecarBody,
}

/// 付属ファイルの内容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SidecarBody {
    Transcript(Transcript),
    Waveform(Waveform),
    PreviewMedia(PreviewMedia),
    /// シーンの区切り。
    Scenes(Scenes),
}

/// 文字起こし。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transcript {
    /// ISO 639-1 の言語コード（`ja`、`en`）。
    pub language: String,
    pub segments: Vec<TranscriptSegment>,
}

/// 文字起こしの 1 区間。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptSegment {
    pub start: RationalTime,
    pub end: RationalTime,
    pub text: String,
    /// 単語ごとの時刻。取れないツールは省く。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<TranscriptWord>,
}

/// 文字起こしの 1 単語。単語の ID は「素材のコンテンツハッシュ + 開始の時刻」から決まる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptWord {
    pub start: RationalTime,
    pub end: RationalTime,
    pub text: String,
}

/// 波形。ピークは -1..=1。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Waveform {
    pub peaks_per_second: u32,
    pub channels: u16,
    /// チャンネルごとにインターリーブしたピーク。
    pub peaks: Vec<f32>,
}

/// プレビュー動画の記録。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PreviewMedia {
    /// `cache/` からの相対パス。
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// シーンの区切り。素材の先頭からの時刻の昇順。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scenes {
    pub cuts: Vec<SceneCut>,
}

/// シーンが変わる時刻。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneCut {
    pub time: RationalTime,
    /// 変わり方の大きさ（0..=1）。ツールが出さないときは省く。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

/// 凍結したスキーマの置き場所（クレートのルートからの相対パス）。テストが最新かを確かめる。
pub const SCHEMA_PATH: &str = "schema/sidecar.schema.json";

/// 付属ファイルの JSON Schema を決定的な JSON の文字列にする（インデント 2、末尾に改行 1 つ）。
/// 外のツール（`tools/`）はこの形で書く。
pub fn sidecar_schema_json() -> String {
    let mut s = serde_json::to_string_pretty(&schemars::schema_for!(Sidecar))
        .expect("スキーマは JSON になる");
    s.push('\n');
    s
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
