//! `<project>/cache/` の実装（`mm_cache_api::CacheStore`）。
//!
//! - 置き場所: `<root>/<種類>/<contentHash>/<variant>`。`contentHash` で引くので、プロジェクトを別の場所に
//!   コピーしても、素材が同じなら当たる。`cache/` を丸ごと消しても、次に開いたときに作り直すだけ。
//! - 触った順: メモリの索引が持つ。開き直したときはファイルの更新時刻の順で復元し、`get` のたびに更新時刻も進める。
//! - 書き込み: 一時ファイルに書いてから名前を付け替えるので、途中で落ちても壊れたファイルは残らない。
//!   開くときは 1 時間より古い一時ファイルだけ捨てる（別のプロセスが書いている最中のものは残す）。
//! - 大きさ: 上限を超えたら、触った順に古いものから消す。上限より大きい 1 つは置いたそばから消える。

mod layout;

use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use mm_cache_api::{CacheKey, CacheKind, CacheStore};
use mm_types::{MmError, Result};

use crate::layout::{entry_path, parse_entry, TMP_DIR};

/// これより古い一時ファイルは、書いたプロセスが落ちて残ったものとして捨てる。
const STALE_TMP_AGE: Duration = Duration::from_secs(60 * 60);

/// 索引の 1 項目。
#[derive(Debug, Clone, Copy)]
struct Entry {
    size: u64,
    /// 触った順。大きいほど新しい。
    tick: u64,
}

#[derive(Debug, Default)]
struct Index {
    entries: HashMap<CacheKey, Entry>,
    size: u64,
    tick: u64,
}

impl Index {
    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    fn insert(&mut self, key: CacheKey, size: u64) {
        let tick = self.next_tick();
        if let Some(old) = self.entries.insert(key, Entry { size, tick }) {
            self.size -= old.size;
        }
        self.size += size;
    }

    fn take(&mut self, key: &CacheKey) -> Option<Entry> {
        let entry = self.entries.remove(key)?;
        self.size -= entry.size;
        Some(entry)
    }

    /// 一番古いキー。
    fn oldest(&self) -> Option<CacheKey> {
        self.entries
            .iter()
            .min_by_key(|(_, e)| e.tick)
            .map(|(k, _)| k.clone())
    }
}

/// ディスクのキャッシュ。
#[derive(Debug)]
pub struct FsCache {
    root: PathBuf,
    limit: Option<u64>,
    index: Mutex<Index>,
    /// 一時ファイルの名前の通し番号。名前は pid と組にして、同じ `cache/` を開く別プロセスと被らせない。
    seq: AtomicU64,
}

impl FsCache {
    /// `root` を開く（無ければ作る）。既にあるファイルを索引に読み、上限を超えていたら古いものから消す。
    pub fn open(root: impl Into<PathBuf>, limit: Option<u64>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join(TMP_DIR))?;
        let cache = Self {
            index: Mutex::new(scan(&root)?),
            root,
            limit,
            seq: AtomicU64::new(0),
        };
        cache.discard_stale_tmp();
        {
            let mut index = cache.lock();
            cache.evict(&mut index);
        }
        Ok(cache)
    }

    /// 置き場所のルート。
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock(&self) -> MutexGuard<'_, Index> {
        self.index.lock().expect("cache index lock")
    }

    /// 別のプロセスが書いている最中の一時ファイルを消さないよう、古いものだけ捨てる。
    fn discard_stale_tmp(&self) {
        let Ok(dir) = fs::read_dir(self.root.join(TMP_DIR)) else {
            return;
        };
        let now = SystemTime::now();
        for e in dir.flatten() {
            let age = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok());
            if age.is_some_and(|a| a >= STALE_TMP_AGE) {
                let _ = fs::remove_file(e.path());
            }
        }
    }

    /// 上限に収まるまで古いものから消す。
    fn evict(&self, index: &mut Index) {
        let Some(limit) = self.limit else { return };
        while index.size > limit {
            let Some(oldest) = index.oldest() else { break };
            index.take(&oldest);
            let _ = fs::remove_file(entry_path(&self.root, &oldest));
            mm_trace::counter!("cache.evicted_entries", 1);
        }
    }
}

/// 既にあるファイルを索引に読む。更新時刻の順に触った順を付ける。
fn scan(root: &Path) -> Result<Index> {
    let mut found: Vec<(SystemTime, CacheKey, u64)> = Vec::new();
    for kind_dir in read_dirs(root)? {
        for hash_dir in read_dirs(&kind_dir)? {
            let Ok(files) = fs::read_dir(&hash_dir) else {
                continue;
            };
            for file in files.flatten() {
                let Ok(meta) = file.metadata() else { continue };
                let Some(key) = parse_entry(&file.path(), root) else {
                    continue;
                };
                found.push((
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    key,
                    meta.len(),
                ));
            }
        }
    }
    found.sort_by_key(|(modified, _, _)| *modified);
    let mut index = Index::default();
    for (_, key, size) in found {
        index.insert(key, size);
    }
    Ok(index)
}

fn read_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir)?.flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) && e.file_name() != TMP_DIR {
            out.push(e.path());
        }
    }
    Ok(out)
}

fn kind_name(kind: CacheKind) -> &'static str {
    layout::kind_dir(kind)
}

impl CacheStore for FsCache {
    fn id(&self) -> &'static str {
        "fs"
    }

    fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>> {
        let known = {
            let mut index = self.lock();
            let tick = index.next_tick();
            index.entries.get_mut(key).map(|e| e.tick = tick).is_some()
        };
        mm_trace::counter!(format!("cache.fs_{}_lookups", kind_name(key.kind)), 1);
        if !known {
            return Ok(None);
        }
        let path = entry_path(&self.root, key);
        match fs::read(&path) {
            Ok(bytes) => {
                touch_mtime(&path);
                mm_trace::counter!(format!("cache.fs_{}_hits", kind_name(key.kind)), 1);
                Ok(Some(bytes))
            }
            // 外から消された。索引からも外して、無かったことにする。
            Err(e) if e.kind() == ErrorKind::NotFound => {
                self.lock().take(key);
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn put(&self, key: &CacheKey, bytes: &[u8]) -> Result<()> {
        let tmp = self.root.join(TMP_DIR).join(format!(
            "{}-{}.tmp",
            std::process::id(),
            self.seq.fetch_add(1, Ordering::Relaxed)
        ));
        let path = entry_path(&self.root, key);
        let parent = path
            .parent()
            .ok_or_else(|| MmError::Internal("キャッシュの置き場所に親が無い".into()))?;
        fs::create_dir_all(parent)?;
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        drop(file);
        let mut index = self.lock();
        fs::rename(&tmp, &path).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })?;
        index.insert(key.clone(), bytes.len() as u64);
        self.evict(&mut index);
        mm_trace::gauge!("cache.bytes", index.size);
        Ok(())
    }

    fn remove(&self, key: &CacheKey) -> Result<()> {
        let mut index = self.lock();
        index.take(key);
        match fs::remove_file(entry_path(&self.root, key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn size_bytes(&self) -> u64 {
        self.lock().size
    }

    fn limit_bytes(&self) -> Option<u64> {
        self.limit
    }

    fn clear(&self) -> Result<()> {
        let mut index = self.lock();
        for dir in read_dirs(&self.root)? {
            fs::remove_dir_all(dir)?;
        }
        *index = Index::default();
        Ok(())
    }
}

/// 更新時刻を今にする。開き直したときの触った順になる。できなくても索引は合っているので無視する。
fn touch_mtime(path: &Path) {
    if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mm_cache_api::ContentHash;

    use super::*;

    fn key(hash: &str, variant: &str) -> CacheKey {
        CacheKey {
            kind: CacheKind::Frame,
            content_hash: ContentHash(hash.into()),
            variant: variant.into(),
        }
    }

    #[test]
    fn 契約を通る() {
        let dir = tempfile::tempdir().unwrap();
        let n = std::sync::atomic::AtomicU32::new(0);
        mm_testkit::cache::contract(
            |limit| {
                let sub = dir
                    .path()
                    .join(n.fetch_add(1, Ordering::Relaxed).to_string());
                Arc::new(FsCache::open(sub, Some(limit)).unwrap())
            },
            4096,
        );
    }

    #[test]
    fn 開き直しても残り_触った順も保つ() {
        let dir = tempfile::tempdir().unwrap();
        {
            let c = FsCache::open(dir.path(), None).unwrap();
            c.put(&key("h", "old"), &[1; 10]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(30));
            c.put(&key("h", "new"), &[2; 10]).unwrap();
        }
        let c = FsCache::open(dir.path(), Some(15)).unwrap();
        assert_eq!(c.get(&key("h", "old")).unwrap(), None, "古いほうが消える");
        assert_eq!(c.get(&key("h", "new")).unwrap(), Some(vec![2; 10]));
        assert_eq!(c.size_bytes(), 10);
    }

    #[test]
    fn 使えない文字を含むキーも往復する() {
        let dir = tempfile::tempdir().unwrap();
        let k = key("../a:b", "540p/ x?*\"<>|%.");
        {
            let c = FsCache::open(dir.path(), None).unwrap();
            c.put(&k, b"x").unwrap();
        }
        let c = FsCache::open(dir.path(), None).unwrap();
        assert_eq!(c.get(&k).unwrap(), Some(b"x".to_vec()));
        assert!(dir.path().join(".tmp").exists());
        assert!(!dir.path().parent().unwrap().join("a:b").exists());
    }

    #[test]
    fn 外から消されたファイルは_none_になり索引から外れる() {
        let dir = tempfile::tempdir().unwrap();
        let c = FsCache::open(dir.path(), None).unwrap();
        let k = key("h", "v");
        c.put(&k, &[0; 8]).unwrap();
        fs::remove_file(entry_path(dir.path(), &k)).unwrap();
        assert_eq!(c.get(&k).unwrap(), None);
        assert_eq!(c.size_bytes(), 0);
    }

    #[test]
    fn 上限より大きい_1_つは置いたそばから消える() {
        let dir = tempfile::tempdir().unwrap();
        let c = FsCache::open(dir.path(), Some(4)).unwrap();
        c.put(&key("h", "big"), &[0; 8]).unwrap();
        assert_eq!(c.size_bytes(), 0);
        assert_eq!(c.get(&key("h", "big")).unwrap(), None);
    }

    #[test]
    fn 古い一時ファイルだけ開くときに捨てる() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join(".tmp");
        fs::create_dir_all(&tmp).unwrap();
        let old = tmp.join("1-0.tmp");
        let fresh = tmp.join("2-0.tmp");
        fs::write(&old, b"half").unwrap();
        fs::write(&fresh, b"writing").unwrap();
        let long_ago = SystemTime::now() - STALE_TMP_AGE - Duration::from_secs(60);
        fs::OpenOptions::new()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        let c = FsCache::open(dir.path(), None).unwrap();
        assert_eq!(c.size_bytes(), 0);
        assert!(!old.exists(), "古い一時ファイルは捨てる");
        assert!(fresh.exists(), "書いている最中かもしれないものは残す");
    }

    #[test]
    fn 同じ置き場を開く二つのキャッシュは一時ファイルを壊し合わない() {
        let dir = tempfile::tempdir().unwrap();
        let a = FsCache::open(dir.path(), None).unwrap();
        // 別プロセスの書き込み中を模す: 自分とは違う pid の名前で置く。
        let other = dir
            .path()
            .join(".tmp")
            .join(format!("{}-0.tmp", std::process::id() + 1));
        fs::write(&other, b"writing").unwrap();
        let b = FsCache::open(dir.path(), None).unwrap();
        assert!(other.exists(), "後から開いても他の一時ファイルは消えない");
        a.put(&key("h", "a"), &[1; 4]).unwrap();
        b.put(&key("h", "b"), &[2; 4]).unwrap();
        assert_eq!(a.get(&key("h", "a")).unwrap(), Some(vec![1; 4]));
        assert_eq!(b.get(&key("h", "b")).unwrap(), Some(vec![2; 4]));
    }
}
