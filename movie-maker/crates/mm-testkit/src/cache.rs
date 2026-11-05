//! キャッシュの見本と契約テスト。見本はメモリの中に置き、触った順で古いものから消す。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mm_cache_api::{CacheKey, CacheKind, CacheStore, ContentHash};
use mm_types::Result;

#[derive(Debug, Default)]
struct Inner {
    entries: HashMap<CacheKey, (Vec<u8>, u64)>,
    tick: u64,
    size: u64,
}

impl Inner {
    fn touch(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    fn evict_until(&mut self, limit: u64) {
        while self.size > limit {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.remove(&oldest);
        }
    }

    fn remove(&mut self, key: &CacheKey) {
        if let Some((bytes, _)) = self.entries.remove(key) {
            self.size -= bytes.len() as u64;
        }
    }
}

/// 見本のキャッシュ。
#[derive(Debug, Default)]
pub struct MemoryCache {
    limit: Option<u64>,
    inner: Mutex<Inner>,
}

impl MemoryCache {
    /// 上限つき。`None` は無制限。
    pub fn new(limit: Option<u64>) -> Self {
        Self {
            limit,
            inner: Mutex::default(),
        }
    }
}

impl CacheStore for MemoryCache {
    fn id(&self) -> &'static str {
        "memory"
    }

    fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>> {
        let mut inner = self.inner.lock().expect("cache lock");
        let tick = inner.touch();
        Ok(inner.entries.get_mut(key).map(|(bytes, t)| {
            *t = tick;
            bytes.clone()
        }))
    }

    fn put(&self, key: &CacheKey, bytes: &[u8]) -> Result<()> {
        let mut inner = self.inner.lock().expect("cache lock");
        inner.remove(key);
        let tick = inner.touch();
        inner.size += bytes.len() as u64;
        inner.entries.insert(key.clone(), (bytes.to_vec(), tick));
        if let Some(limit) = self.limit {
            inner.evict_until(limit);
        }
        Ok(())
    }

    fn remove(&self, key: &CacheKey) -> Result<()> {
        self.inner.lock().expect("cache lock").remove(key);
        Ok(())
    }

    fn size_bytes(&self) -> u64 {
        self.inner.lock().expect("cache lock").size
    }

    fn limit_bytes(&self) -> Option<u64> {
        self.limit
    }

    fn clear(&self) -> Result<()> {
        let mut inner = self.inner.lock().expect("cache lock");
        inner.entries.clear();
        inner.size = 0;
        Ok(())
    }
}

fn key(hash: &str, variant: &str) -> CacheKey {
    CacheKey {
        kind: CacheKind::Frame,
        content_hash: ContentHash(hash.into()),
        variant: variant.into(),
    }
}

/// キャッシュの契約。`make` は上限 `limit` バイトの置き場を作る。
pub fn contract(make: impl Fn(u64) -> Arc<dyn CacheStore>, limit: u64) {
    let store = make(limit);
    assert_eq!(store.limit_bytes(), Some(limit));
    let unit = (limit / 4).max(1) as usize;

    assert_eq!(store.get(&key("a", "0")).unwrap(), None, "無いキーは None");
    store.put(&key("a", "0"), &vec![1u8; unit]).unwrap();
    assert_eq!(
        store.get(&key("a", "0")).unwrap(),
        Some(vec![1u8; unit]),
        "置く → 引ける"
    );
    store.remove(&key("a", "0")).unwrap();
    assert_eq!(store.get(&key("a", "0")).unwrap(), None, "消した後は None");

    for i in 0..6 {
        store
            .put(&key("a", &i.to_string()), &vec![i as u8; unit])
            .unwrap();
    }
    assert!(store.size_bytes() <= limit, "上限に収まる");
    assert_eq!(
        store.get(&key("a", "0")).unwrap(),
        None,
        "古いものから消える"
    );
    assert!(
        store.get(&key("a", "5")).unwrap().is_some(),
        "新しいものは残る"
    );

    store.clear().unwrap();
    assert_eq!(store.size_bytes(), 0, "clear で 0");

    store.put(&key("same-hash", "x"), b"copy").unwrap();
    let other = make(limit);
    other.put(&key("same-hash", "x"), b"copy").unwrap();
    assert_eq!(
        store.get(&key("same-hash", "x")).unwrap(),
        other.get(&key("same-hash", "x")).unwrap(),
        "別の置き場でも同じ contentHash で当たる"
    );
}
