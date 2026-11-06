//! フレームのキャッシュの入口。レイヤーの画像（`kind_image` の結果）と入れ子のコンポジションの画像
//! （`nested_image` の結果）を [`memo::Memo`] に覚え、GPU のテクスチャのまま使い回す。
//!
//! - 持ち主: 覚えたテクスチャはこのキャッシュのもの。呼んだ側は `temps` に入れず、解放もしない。
//!   追い出したテクスチャは `graveyard` に置き、次のフレームの前（`flush`）に解放する。同じフレームの
//!   合成が参照している途中のテクスチャを手放さないため。
//! - ディスク: 2 回以上使われた画像が上限で追い出されたとき、`CacheStore` に half float のまま退避する（[`disk`]）。
//!   1 回も使われなかった画像（スクラブで通り過ぎたフレーム）は読み戻しが無駄なので退避しない。
//! - 捨てる: 操作が触った ID に頼った画像だけ。キーに評価済みのパラメータと素材のハッシュが入っているので（[`key`]）、
//!   パラメータや素材の中身が変わった画像は引かれなくなるだけで、捨てるのは場所を空けるため。

mod disk;
mod key;
mod memo;

use std::sync::Arc;

use mm_cache_api::CacheStore;
use mm_gpu::{WgpuGpu, FRAME_FORMAT};
use mm_node_api::GpuHelper;
use mm_render_api::FrameCacheKey;
use mm_types::{Rect, Size};

use disk::Spiller;
pub(crate) use key::{kind_key, nested_key, KeyEnv, Keyed, NestedKeyInput};
pub use memo::MemoStats;
use memo::{Dropped, Evicted, Memo, Weighted};

/// メモリ（VRAM）に置く画像の合計の既定の上限。1080p の half float で約 60 枚。
pub const DEFAULT_MEMORY_BYTES: u64 = 1 << 30;
/// 画素あたりのバイト数（Rgba16Float）。
const BYTES_PER_PIXEL: u64 = 8;
/// 退避に回す、追い出されるまでに当たった回数の下限。
const SPILL_MIN_HITS: u32 = 1;

/// フレームのキャッシュの設定。
#[derive(Clone)]
pub struct FrameCacheConfig {
    /// メモリに置く画像の合計の上限（バイト）。0 でキャッシュを使わない。
    pub memory_bytes: u64,
    /// 追い出した画像の退避先。`None` ならメモリだけ。
    pub store: Option<Arc<dyn CacheStore>>,
}

impl Default for FrameCacheConfig {
    fn default() -> Self {
        Self {
            memory_bytes: DEFAULT_MEMORY_BYTES,
            store: None,
        }
    }
}

/// 覚えた画像。テクスチャはキャッシュが持つ。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CachedImage {
    pub texture: u64,
    pub size: Size,
    pub place: Rect,
}

impl Weighted for CachedImage {
    fn weight_bytes(&self) -> u64 {
        self.size.area() as u64 * BYTES_PER_PIXEL
    }
}

/// 覚えと退避をまとめたもの。
pub(crate) struct RenderCache {
    memo: Memo<CachedImage>,
    spiller: Option<Spiller>,
    /// 追い出したが、まだテクスチャを解放していない画像。
    graveyard: Vec<Evicted<CachedImage>>,
}

impl RenderCache {
    pub fn new(config: FrameCacheConfig) -> Self {
        Self {
            memo: Memo::new(config.memory_bytes),
            spiller: config.store.map(Spiller::new),
            graveyard: Vec::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.memo.limit_bytes() > 0
    }

    pub fn stats(&self) -> MemoStats {
        self.memo.stats()
    }

    /// 引く。メモリに無ければ退避先を見る。
    pub fn lookup(
        &mut self,
        key: &FrameCacheKey,
        deps: &[String],
        gpu: &mut dyn GpuHelper,
    ) -> Option<CachedImage> {
        mm_trace::counter!("cache.frame_lookups", 1);
        if let Some(image) = self.memo.get(key) {
            mm_trace::counter!("cache.frame_hits", 1);
            return Some(*image);
        }
        let image = self.spiller.as_ref()?.get(key, gpu)?;
        mm_trace::counter!("cache.frame_hits", 1);
        mm_trace::counter!("cache.frame_disk_hits", 1);
        self.insert(key.clone(), deps.to_vec(), image);
        Some(image)
    }

    /// 覚える。テクスチャの持ち主はキャッシュに移る。
    pub fn insert(&mut self, key: FrameCacheKey, deps: Vec<String>, image: CachedImage) {
        let evicted = self.memo.insert(key, deps, image);
        self.graveyard.extend(evicted);
        self.publish_size();
    }

    /// 描いた画像を覚える。覚えたら、以後のテクスチャの持ち主はキャッシュで、その画像を返す。
    /// `owned` が偽（デコーダーの借用など他の持ち主）のテクスチャは GPU の上でコピーしてから覚え、
    /// 元は手放す。コピーできない形式や、キーが無い（覚えない）ときは `None`で、元は呼んだ側のまま。
    pub fn remember(
        &mut self,
        key: Option<Keyed>,
        image: CachedImage,
        owned: bool,
        gpu: &mut WgpuGpu,
    ) -> Option<CachedImage> {
        let (key, deps) = key?;
        if owned {
            self.insert(key, deps, image);
            return Some(image);
        }
        let texture = snapshot(gpu, image.texture, image.size)?;
        gpu.release_id(image.texture);
        let copy = CachedImage { texture, ..image };
        self.insert(key, deps, copy);
        Some(copy)
    }

    /// `ids` に頼った画像を捨てる。テクスチャは次の `flush` で解放する。
    pub fn invalidate(&mut self, ids: &[String]) {
        let dropped = self.memo.invalidate(ids);
        mm_trace::counter!("cache.frame_invalidated", dropped.len());
        let bytes: u64 = dropped.iter().map(|e| e.value.weight_bytes()).sum();
        mm_trace::counter!("cache.frame_invalidated_bytes", bytes);
        self.graveyard.extend(dropped);
        self.publish_size();
    }

    fn publish_size(&self) {
        let stats = self.memo.stats();
        mm_trace::gauge!("cache.frame_memory_bytes", stats.bytes);
        mm_trace::gauge!("cache.frame_entries", stats.entries);
    }

    /// 全部捨てる。テクスチャは次の `flush` で解放する。
    pub fn clear(&mut self) {
        let dropped = self.memo.clear();
        self.graveyard.extend(dropped);
    }

    /// GPU のデバイスを作り直したとき。テクスチャは前のデバイスのものなので解放しない。
    pub fn forget_all(&mut self) {
        self.memo.clear();
        self.graveyard.clear();
    }

    /// 追い出した画像を、退避するものは写す命令を出してから解放する。フレームを描き始める前に呼ぶ。
    /// 写し終えるのを待つのとファイルへの書き込みは別のスレッドで行うので、ここでは止まらない。
    pub fn flush(&mut self, gpu: &mut WgpuGpu) {
        for evicted in std::mem::take(&mut self.graveyard) {
            let spill = evicted.reason == Dropped::Capacity && evicted.hits >= SPILL_MIN_HITS;
            if let (true, Some(spiller)) = (spill, self.spiller.as_mut()) {
                spiller.spill(gpu, evicted.key, evicted.value);
            }
            gpu.release_id(evicted.value.texture);
        }
    }

    /// ここまでに頼んだ退避の書き込みが終わるまで待つ。
    pub fn wait_spills(&mut self) {
        if let Some(spiller) = self.spiller.as_mut() {
            spiller.wait();
        }
    }
}

/// テクスチャを自分のものにコピーする。形式が `FRAME_FORMAT` でない、またはコピー元にできないテクスチャは `None`。
fn snapshot(gpu: &mut WgpuGpu, id: u64, size: Size) -> Option<u64> {
    let src = gpu.texture(id)?;
    let copyable = src.format() == FRAME_FORMAT
        && src.usage().contains(wgpu::TextureUsages::COPY_SRC)
        && src.width() == size.width
        && src.height() == size.height;
    if !copyable {
        return None;
    }
    let copy = gpu.create(size).ok()?;
    let (src, dst) = (gpu.texture(id)?, gpu.texture(copy)?);
    let ctx = gpu.context();
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render.cache_copy"),
        });
    encoder.copy_texture_to_texture(
        src.as_image_copy(),
        dst.as_image_copy(),
        wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
    );
    // 借用したフレームはデコードの完了を待ってから読む。
    gpu.before_submit();
    ctx.queue.submit([encoder.finish()]);
    Some(copy)
}
