//! PTS の索引のキャッシュ。素材を開くたびに全パケットを読む（1 時間の 4K 50 Mbps で約 22 GB）のをやめ、
//! 1 回目に作った索引を `cache/` に置いて、2 回目からは読まない。
//!
//! - 引く名前: 素材の指紋（大きさと、先頭・中央・末尾の 64 KiB の BLAKE3）とストリーム番号。パスに依らないので、
//!   素材を動かしても、プロジェクトを別の場所にコピーしても当たる。指紋は素材の全体を読まずに計算できる
//!   （`Asset.contentHash` は全体を読む取り込みのハッシュで、デコーダーには渡らない）。
//! - コンテナの索引（mov の `index_entries` など）を使わない理由: エントリーの時刻は DTS で、B フレームの
//!   PTS には composition offset が要り、パケットを読まずには得られない。mkv の cues は粗くて全フレームに足りない。
//! - 置き場所は処理全体で 1 つ（`set_index_cache`）。3 つのデコーダー（ffmpeg、d3d12va、cuvid）が同じ
//!   `Timeline::build` を通るので、デコーダーのインターフェースを変えずに全部に効く。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, RwLock};

use mm_cache_api::{CacheKey, CacheKind, CacheStore, ContentHash};
use mm_types::Result;

use crate::timeline::{scan_video, VideoScan};

/// 索引の形を変えたら上げる（古い索引は引かれなくなる）。
const INDEX_VERSION: u32 = 1;
/// 指紋に使う、素材の 1 か所あたりのバイト数。
const SAMPLE_BYTES: u64 = 64 * 1024;
/// 指紋に使う、素材の中の場所の数（先頭、中央、末尾）。
const SAMPLE_SPOTS: u64 = 3;

static STORE: RwLock<Option<Arc<dyn CacheStore>>> = RwLock::new(None);

/// 索引を置く場所を決める。`None` で置かない（毎回読む）。
pub fn set_index_cache(store: Option<Arc<dyn CacheStore>>) {
    *STORE.write().expect("index cache lock") = store;
}

fn store() -> Option<Arc<dyn CacheStore>> {
    STORE.read().expect("index cache lock").clone()
}

/// `scan_video` の結果を `cache/` から引く。無ければ読んで置く。
pub fn scan_video_cached(path: &Path, stream_index: usize) -> Result<VideoScan> {
    let Some(store) = store() else {
        return scan_video(path, stream_index);
    };
    let key = fingerprint(path).map(|hash| CacheKey {
        kind: CacheKind::Intermediate,
        content_hash: ContentHash(hash),
        variant: format!("video-index-v{INDEX_VERSION}-s{stream_index}"),
    });
    let Ok(key) = key else {
        return scan_video(path, stream_index);
    };
    if let Some(scan) = store
        .get(&key)
        .ok()
        .flatten()
        .and_then(|b| VideoScan::from_bytes(&b))
    {
        mm_trace::counter!("decode.index_cache_hits", 1);
        return Ok(scan);
    }
    let scan = scan_video(path, stream_index)?;
    if let Err(e) = store.put(&key, &scan.to_bytes()) {
        tracing::warn!(error = %e, "PTS の索引をキャッシュに置けない");
    }
    Ok(scan)
}

/// 素材の指紋。大きさと、先頭・中央・末尾の `SAMPLE_BYTES`。
fn fingerprint(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&len.to_le_bytes());
    let mut buf = vec![0u8; SAMPLE_BYTES as usize];
    for spot in 0..SAMPLE_SPOTS {
        let at = len.saturating_sub(SAMPLE_BYTES) * spot / (SAMPLE_SPOTS - 1);
        file.seek(SeekFrom::Start(at))?;
        let n = file.read(&mut buf)?;
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

impl VideoScan {
    /// キャッシュに置く形。8 バイトの整数の並び（バージョン、フレーム数、最後の長さ、キーフレーム数、PTS、キーフレーム）。
    pub fn to_bytes(&self) -> Vec<u8> {
        let words = 4 + self.raw_pts.len() + self.keyframes.len();
        let mut out = Vec::with_capacity(words * 8);
        let header = [
            u64::from(INDEX_VERSION),
            self.raw_pts.len() as u64,
            self.last_duration as u64,
            self.keyframes.len() as u64,
        ];
        for w in header {
            out.extend_from_slice(&w.to_le_bytes());
        }
        for v in self.raw_pts.iter().chain(&self.keyframes) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// `to_bytes` の逆。形が合わない、PTS が昇順でない、キーフレームの番号が範囲外のものは `None`。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let words: Vec<i64> = bytes
            .chunks_exact(8)
            .map(|c| i64::from_le_bytes(c.try_into().expect("8 バイト")))
            .collect();
        if !bytes.len().is_multiple_of(8) || words.len() < 4 || words[0] != i64::from(INDEX_VERSION)
        {
            return None;
        }
        let (count, last_duration, keys) = (
            usize::try_from(words[1]).ok()?,
            words[2],
            usize::try_from(words[3]).ok()?,
        );
        if words.len() != 4 + count.checked_add(keys)? || count == 0 {
            return None;
        }
        let raw_pts = words[4..4 + count].to_vec();
        let keyframes = words[4 + count..].to_vec();
        let sorted = raw_pts.windows(2).all(|w| w[0] < w[1]);
        let in_range = keyframes.iter().all(|k| (0..count as i64).contains(k));
        (sorted && in_range).then_some(Self {
            raw_pts,
            last_duration,
            keyframes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan() -> VideoScan {
        VideoScan {
            raw_pts: vec![0, 512, 1024, 2048],
            last_duration: 512,
            keyframes: vec![0, 2],
        }
    }

    #[test]
    fn 索引は往復する() {
        let s = scan();
        let back = VideoScan::from_bytes(&s.to_bytes()).unwrap();
        assert_eq!(
            (back.raw_pts, back.last_duration, back.keyframes),
            (s.raw_pts, s.last_duration, s.keyframes)
        );
    }

    #[test]
    fn 壊れた索引は引かなかったことにする() {
        let good = scan().to_bytes();
        assert!(VideoScan::from_bytes(&good[..good.len() - 1]).is_none());
        assert!(VideoScan::from_bytes(&good[..good.len() - 8]).is_none());
        let mut bad_version = good.clone();
        bad_version[0] = 99;
        assert!(VideoScan::from_bytes(&bad_version).is_none());
        let unsorted = VideoScan {
            raw_pts: vec![5, 3],
            ..scan()
        };
        assert!(VideoScan::from_bytes(&unsorted.to_bytes()).is_none());
        let key_out = VideoScan {
            keyframes: vec![9],
            ..scan()
        };
        assert!(VideoScan::from_bytes(&key_out.to_bytes()).is_none());
        assert!(VideoScan::from_bytes(&[]).is_none());
    }

    #[test]
    fn 指紋は中身で決まりパスに依らない() {
        let dir = std::env::temp_dir().join(format!("mm-fp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a.bin"), dir.join("b.bin"), dir.join("c.bin"));
        let body: Vec<u8> = (0..400_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&a, &body).unwrap();
        std::fs::write(&b, &body).unwrap();
        let mut changed = body.clone();
        *changed.last_mut().unwrap() ^= 1;
        std::fs::write(&c, &changed).unwrap();
        assert_eq!(fingerprint(&a).unwrap(), fingerprint(&b).unwrap());
        assert_ne!(fingerprint(&a).unwrap(), fingerprint(&c).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
