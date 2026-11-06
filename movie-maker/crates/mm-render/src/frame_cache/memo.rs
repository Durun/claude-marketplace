//! 時刻とキーで引く計算の覚え。rustc のクエリのメモ化に倣い、`FrameCacheKey` で結果を覚え、
//! 結果が頼った ID（レイヤー、素材、コンポジション）のどれかが変わったときに、その結果だけを捨てる。
//!
//! 覚えた値の持ち主はこの型。追い出した値は呼んだ側へ返すので、GPU のテクスチャの解放や、ディスクの
//! キャッシュ（`mm_cache_api::CacheStore`）への退避は呼んだ側が決める。

use std::collections::{BTreeMap, HashMap, HashSet};

use mm_cache_api::{CacheKey, CacheKind, ContentHash};

use mm_render_api::FrameCacheKey;

/// 覚える値の大きさ。メモリの上限の数え方。
pub trait Weighted {
    /// 持っているバイト数。
    fn weight_bytes(&self) -> u64;
}

/// 覚えから外れた理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    /// 上限を超えて、触った順に古いものから外れた。
    Capacity,
    /// 頼った ID が変わった。
    Invalidated,
    /// 同じキーで置き直された。
    Replaced,
}

/// 覚えから外れた項目。
#[derive(Debug)]
pub struct Evicted<V> {
    pub key: FrameCacheKey,
    pub value: V,
    /// 外れるまでに当たった回数。0 なら 1 度も使われなかった。
    pub hits: u32,
    pub reason: Dropped,
}

/// 数え上げ。引いた回数とヒット率を `mm-trace` に載せる元。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoStats {
    pub lookups: u64,
    pub hits: u64,
    pub inserted: u64,
    pub evicted: u64,
    pub invalidated: u64,
    pub entries: usize,
    pub bytes: u64,
}

struct Slot<V> {
    key: FrameCacheKey,
    deps: Vec<String>,
    value: V,
    weight: u64,
    tick: u64,
    hits: u32,
}

/// `FrameCacheKey` で引くメモ。メモリの上限は値の `weight_bytes` の合計。
pub struct Memo<V> {
    limit_bytes: u64,
    bytes: u64,
    tick: u64,
    slots: HashMap<u64, Slot<V>>,
    /// 触った順（古い順）から `digest`。
    order: BTreeMap<u64, u64>,
    by_dep: HashMap<String, HashSet<u64>>,
    stats: MemoStats,
}

impl<V: Weighted> Memo<V> {
    /// 上限つきで作る。
    pub fn new(limit_bytes: u64) -> Self {
        Self {
            limit_bytes,
            bytes: 0,
            tick: 0,
            slots: HashMap::new(),
            order: BTreeMap::new(),
            by_dep: HashMap::new(),
            stats: MemoStats::default(),
        }
    }

    /// メモリの上限（バイト）。
    pub fn limit_bytes(&self) -> u64 {
        self.limit_bytes
    }

    /// 数え上げ。
    pub fn stats(&self) -> MemoStats {
        MemoStats {
            entries: self.slots.len(),
            bytes: self.bytes,
            ..self.stats
        }
    }

    /// 引く。当たれば触った順を新しくする。ダイジェストが同じでもキーが違えば外れ。
    pub fn get(&mut self, key: &FrameCacheKey) -> Option<&V> {
        self.stats.lookups += 1;
        let digest = key.digest();
        let slot = self.slots.get_mut(&digest).filter(|s| s.key == *key)?;
        self.stats.hits += 1;
        self.order.remove(&slot.tick);
        self.tick += 1;
        slot.tick = self.tick;
        slot.hits = slot.hits.saturating_add(1);
        self.order.insert(slot.tick, digest);
        Some(&slot.value)
    }

    /// 置く。`deps` は値が頼った ID。上限を超えた分と、置き直された古い値を返す。
    /// 1 つで上限より大きい値は、置いたそばから返ってくる。
    pub fn insert(&mut self, key: FrameCacheKey, deps: Vec<String>, value: V) -> Vec<Evicted<V>> {
        let digest = key.digest();
        let mut out = Vec::new();
        if let Some(old) = self.remove_slot(digest) {
            out.push(old.into_evicted(Dropped::Replaced));
        }
        let weight = value.weight_bytes();
        self.tick += 1;
        for dep in &deps {
            self.by_dep.entry(dep.clone()).or_default().insert(digest);
        }
        self.order.insert(self.tick, digest);
        self.bytes += weight;
        self.slots.insert(
            digest,
            Slot {
                key,
                deps,
                value,
                weight,
                tick: self.tick,
                hits: 0,
            },
        );
        self.stats.inserted += 1;
        while self.bytes > self.limit_bytes {
            let Some((_, oldest)) = self.order.first_key_value().map(|(t, d)| (*t, *d)) else {
                break;
            };
            if let Some(slot) = self.remove_slot(oldest) {
                self.stats.evicted += 1;
                out.push(slot.into_evicted(Dropped::Capacity));
            }
        }
        out
    }

    /// `ids` のどれかに頼った値を全部捨てて返す。
    pub fn invalidate(&mut self, ids: &[String]) -> Vec<Evicted<V>> {
        let digests: HashSet<u64> = ids
            .iter()
            .filter_map(|id| self.by_dep.get(id))
            .flatten()
            .copied()
            .collect();
        let mut out = Vec::new();
        for digest in digests {
            if let Some(slot) = self.remove_slot(digest) {
                self.stats.invalidated += 1;
                out.push(slot.into_evicted(Dropped::Invalidated));
            }
        }
        out
    }

    /// 全部捨てて返す。
    pub fn clear(&mut self) -> Vec<Evicted<V>> {
        let digests: Vec<u64> = self.slots.keys().copied().collect();
        let mut out = Vec::with_capacity(digests.len());
        for digest in digests {
            if let Some(slot) = self.remove_slot(digest) {
                self.stats.invalidated += 1;
                out.push(slot.into_evicted(Dropped::Invalidated));
            }
        }
        out
    }

    fn remove_slot(&mut self, digest: u64) -> Option<Slot<V>> {
        let slot = self.slots.remove(&digest)?;
        self.order.remove(&slot.tick);
        self.bytes -= slot.weight;
        for dep in &slot.deps {
            if let Some(set) = self.by_dep.get_mut(dep) {
                set.remove(&digest);
                if set.is_empty() {
                    self.by_dep.remove(dep);
                }
            }
        }
        Some(slot)
    }
}

impl<V> Slot<V> {
    fn into_evicted(self, reason: Dropped) -> Evicted<V> {
        Evicted {
            key: self.key,
            value: self.value,
            hits: self.hits,
            reason,
        }
    }
}

/// ディスクのキャッシュ（`CacheStore`）に置くときのキー。素材があるときはその `contentHash` の下に置くので、
/// プロジェクトを別の場所にコピーしても当たる。素材が無いものは `digest` の 16 進の下に置く。
pub(crate) fn store_key(key: &FrameCacheKey) -> CacheKey {
    let digest = format!("{:016x}", key.digest());
    CacheKey {
        kind: CacheKind::Frame,
        content_hash: ContentHash(key.content_hash.clone().unwrap_or_else(|| digest.clone())),
        variant: digest,
    }
}

#[cfg(test)]
mod tests {
    use mm_node_api::Quality;

    use super::*;

    struct Bytes(u64);

    impl Weighted for Bytes {
        fn weight_bytes(&self) -> u64 {
            self.0
        }
    }

    fn key(frame: i64) -> FrameCacheKey {
        FrameCacheKey {
            kind: "footage".into(),
            kind_version: 1,
            renderer_version: 1,
            params_json: "{}".into(),
            input_keys: vec![],
            content_hash: Some("abc".into()),
            frame_index: Some(frame),
            quality: Quality::Preview,
            color_config: "lin".into(),
            size_bucket_milli: 1000,
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn 置いたものは引け_引いた回数と当たった回数を数える() {
        let mut m = Memo::new(100);
        assert!(m.insert(key(1), ids(&["L1"]), Bytes(10)).is_empty());
        assert!(m.get(&key(1)).is_some());
        assert!(m.get(&key(2)).is_none());
        let s = m.stats();
        assert_eq!((s.lookups, s.hits, s.entries, s.bytes), (2, 1, 1, 10));
    }

    #[test]
    fn 上限を超えたら触った順に古いものから外れる() {
        let mut m = Memo::new(30);
        for f in 1..=3 {
            m.insert(key(f), ids(&[]), Bytes(10));
        }
        // 1 を引いて新しくするので、次に外れるのは 2。
        assert!(m.get(&key(1)).is_some());
        let out = m.insert(key(4), ids(&[]), Bytes(10));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, key(2));
        assert_eq!(out[0].reason, Dropped::Capacity);
        assert_eq!(out[0].hits, 0);
        assert!(m.get(&key(2)).is_none());
        assert!(m.get(&key(1)).is_some());
    }

    #[test]
    fn 上限より大きい値は置いたそばから返る() {
        let mut m = Memo::new(10);
        let out = m.insert(key(1), ids(&["L"]), Bytes(11));
        assert_eq!(out.len(), 1);
        assert_eq!(m.stats().bytes, 0);
        assert!(m.get(&key(1)).is_none());
    }

    #[test]
    fn 変わった_id_に頼った値だけが捨てられる() {
        let mut m = Memo::new(1000);
        m.insert(key(1), ids(&["L1", "A1"]), Bytes(10));
        m.insert(key(2), ids(&["L2", "A1"]), Bytes(10));
        m.insert(key(3), ids(&["L3"]), Bytes(10));
        let out = m.invalidate(&ids(&["L1", "L3"]));
        let mut gone: Vec<i64> = out.iter().map(|e| e.key.frame_index.unwrap()).collect();
        gone.sort();
        assert_eq!(gone, vec![1, 3]);
        assert!(out.iter().all(|e| e.reason == Dropped::Invalidated));
        assert!(m.get(&key(2)).is_some(), "頼っていない値は残る");
        assert_eq!(m.stats().bytes, 10);
        // 共有した ID の索引も片付く。
        assert_eq!(m.invalidate(&ids(&["A1"])).len(), 1);
        assert_eq!(m.stats().entries, 0);
    }

    #[test]
    fn 同じキーの置き直しは古い値を返す() {
        let mut m = Memo::new(100);
        m.insert(key(1), ids(&["L1"]), Bytes(10));
        let out = m.insert(key(1), ids(&["L2"]), Bytes(20));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].reason, Dropped::Replaced);
        assert_eq!(m.stats().bytes, 20);
        assert!(m.invalidate(&ids(&["L1"])).is_empty(), "古い依存は残らない");
    }

    #[test]
    fn clear_は全部返す() {
        let mut m = Memo::new(100);
        m.insert(key(1), ids(&["L"]), Bytes(10));
        m.insert(key(2), ids(&["L"]), Bytes(10));
        assert_eq!(m.clear().len(), 2);
        assert_eq!(m.stats().bytes, 0);
    }

    #[test]
    fn ディスクのキーは素材の_content_hash_の下に置く() {
        let k = store_key(&key(7));
        assert_eq!(k.content_hash.0, "abc");
        assert_eq!(k.variant, format!("{:016x}", key(7).digest()));
        let mut none = key(7);
        none.content_hash = None;
        assert_eq!(store_key(&none).content_hash.0, store_key(&none).variant);
    }
}
