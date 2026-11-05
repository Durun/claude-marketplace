//! `cache/` の口。消しても作り直せるものだけを置き、素材の中身のハッシュで引く。
//!
//! 契約（`mm_testkit::cache::contract`）:
//! - `put` → `get` で同じバイト列。
//! - 無いキーは `Ok(None)`。
//! - `remove` した後は `None`。
//! - `limit_bytes` を超えて `put` すると、古いもの（最後に触った順）から消え、`size_bytes` が上限に収まる。
//! - `clear` の後は `size_bytes() == 0`。
//! - キーは `content_hash` で引くので、プロジェクトを別の場所に写しても当たる。

use std::fmt;

use serde::{Deserialize, Serialize};

use mm_types::Result;

/// キャッシュの種類。`cache/<種類>/...` の置き場に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    Preview,
    Frame,
    Intermediate,
    AudioPcm,
    TextRaster,
    Waveform,
    Thumbnail,
}

/// 素材の中身のハッシュ（16 進）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(pub String);

impl ContentHash {
    /// スパンのフィールドに付ける先頭 8 桁。
    pub fn short(&self) -> &str {
        let end = self.0.len().min(8);
        &self.0[..end]
    }
}

/// キャッシュのキー。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheKey {
    pub kind: CacheKind,
    pub content_hash: ContentHash,
    /// 同じ素材の中の区別（`540p`、コマの番号、文字の絵のハッシュ）。
    pub variant: String,
}

impl fmt::Display for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}/{}/{}",
            self.kind, self.content_hash.0, self.variant
        )
    }
}

/// キャッシュの置き場。
pub trait CacheStore: Send + Sync {
    /// 実装の名前（`fs`、見本は `memory`）。
    fn id(&self) -> &'static str;
    /// 引く。無ければ `None`。触った順を更新する。
    fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>>;
    /// 置く。上限を超えたら古いものから消す。
    fn put(&self, key: &CacheKey, bytes: &[u8]) -> Result<()>;
    /// 消す。無くても `Ok`。
    fn remove(&self, key: &CacheKey) -> Result<()>;
    /// 今の大きさ。
    fn size_bytes(&self) -> u64;
    /// 大きさの上限。`None` は無制限。
    fn limit_bytes(&self) -> Option<u64>;
    /// 全部消す。
    fn clear(&self) -> Result<()>;
}
