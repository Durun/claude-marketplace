//! フレームのキャッシュの入口。レイヤーの画像（`kind_image` の結果）と入れ子のコンポジションの画像
//! （`nested_image` の結果）を `mm_render_api::Memo` に覚え、GPU のテクスチャのまま使い回す。
//!
//! - 持ち主: 覚えたテクスチャはこのキャッシュのもの。呼んだ側は `temps` に入れず、解放もしない。
//!   追い出したテクスチャは `graveyard` に置き、次のフレームの前（`flush`）に解放する。同じフレームの
//!   合成が参照している途中のテクスチャを手放さないため。
//! - ディスク: 2 回以上使われた画像が上限で追い出されたとき、`CacheStore` に half float のまま退避する。
//!   1 回も使われなかった画像（スクラブで通り過ぎたフレーム）は読み戻しの待ちが無駄なので退避しない。
//! - 捨てる: 操作が触った ID に頼った画像だけ。キーに評価済みのパラメータと素材のハッシュが入っているので、
//!   パラメータや素材の中身が変わった画像は引かれなくなるだけで、捨てるのは場所を空けるため。

use std::collections::BTreeSet;
use std::sync::Arc;

use half::f16;
use mm_cache_api::CacheStore;
use mm_doc::{Composition, Layer, Project};
use mm_eval::source_time;
use mm_gpu::{WgpuGpu, FRAME_FORMAT};
use mm_node_api::{FrameCx, FrameRef, GpuHelper, GpuTexture, NodeKind, NodeRegistry, Quality};
use mm_render_api::{
    digest_bytes, size_bucket, Dropped, Evicted, FrameCacheKey, Memo, MemoStats, Weighted,
};
use mm_types::{
    AssetId, CompositionId, Id, ParamValue, Params, RationalTime, Rect, RgbaImage, Size,
};

use crate::layers::{evaluate, MAX_DEPTH, NESTED_KIND, NESTED_PARAM};
use crate::renderer::WGPU_RENDERER_VERSION;

/// メモリ（VRAM）に置く画像の合計の既定の上限。1080p の half float で約 60 枚。
pub const DEFAULT_MEMORY_BYTES: u64 = 1 << 30;
/// 画素あたりのバイト数（Rgba16Float）。
const BYTES_PER_PIXEL: u64 = 8;
/// 退避したファイルの先頭。形を変えたら変える。
const MAGIC: &[u8; 4] = b"MMF1";
/// 先頭 + 画像の大きさ（u32 × 2）+ 置き場所（f32 × 4）。
const HEADER_BYTES: usize = 4 + 8 + 16;
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
    store: Option<Arc<dyn CacheStore>>,
    /// 追い出したが、まだテクスチャを解放していない画像。
    graveyard: Vec<Evicted<CachedImage>>,
}

impl RenderCache {
    pub fn new(config: FrameCacheConfig) -> Self {
        Self {
            memo: Memo::new(config.memory_bytes),
            store: config.store,
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
        let bytes = self.store.as_ref()?.get(&key.store_key()).ok().flatten()?;
        let image = decode(&bytes, gpu)?;
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

    /// 追い出した画像を、退避するものは退避してから解放する。フレームを描き始める前に呼ぶ。
    pub fn flush(&mut self, gpu: &mut WgpuGpu) {
        for evicted in std::mem::take(&mut self.graveyard) {
            let spill = evicted.reason == Dropped::Capacity && evicted.hits >= SPILL_MIN_HITS;
            if let (true, Some(store)) = (spill, &self.store) {
                self.spill(store.as_ref(), &evicted, gpu);
            }
            gpu.release_id(evicted.value.texture);
        }
    }

    fn spill(&self, store: &dyn CacheStore, evicted: &Evicted<CachedImage>, gpu: &mut WgpuGpu) {
        let image = evicted.value;
        let frame = FrameRef::Gpu(GpuTexture {
            id: image.texture,
            size: image.size,
        });
        let Ok(read) = gpu.download(&frame) else {
            return;
        };
        let bytes = encode(&image, &read);
        match store.put(&evicted.key.store_key(), &bytes) {
            Ok(()) => mm_trace::counter!("cache.frame_spilled_bytes", bytes.len()),
            Err(e) => tracing::warn!(error = %e, "フレームのキャッシュを退避できない"),
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

fn encode(image: &CachedImage, pixels: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_BYTES + pixels.data().len() * 2);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&image.size.width.to_le_bytes());
    out.extend_from_slice(&image.size.height.to_le_bytes());
    let p = image.place;
    for v in [p.x, p.y, p.width, p.height] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in pixels.data() {
        out.extend_from_slice(&f16::from_f32(*v).to_le_bytes());
    }
    out
}

/// 退避したファイルを GPU に上げ直す。形が合わなければ `None`（引かなかったことにする）。
fn decode(bytes: &[u8], gpu: &mut dyn GpuHelper) -> Option<CachedImage> {
    let head = bytes.get(..HEADER_BYTES).filter(|h| &h[..4] == MAGIC)?;
    let word = |at: usize| head[at..at + 4].try_into().ok();
    let size = Size::new(u32::from_le_bytes(word(4)?), u32::from_le_bytes(word(8)?));
    let place = Rect::new(
        f32::from_le_bytes(word(12)?),
        f32::from_le_bytes(word(16)?),
        f32::from_le_bytes(word(20)?),
        f32::from_le_bytes(word(24)?),
    );
    let body = &bytes[HEADER_BYTES..];
    if body.len() != size.area() * BYTES_PER_PIXEL as usize {
        return None;
    }
    let data: Vec<f32> = body
        .chunks_exact(2)
        .map(|c| f16::from_le_bytes([c[0], c[1]]).to_f32())
        .collect();
    let pixels = RgbaImage::from_raw(size, data).ok()?;
    match gpu.upload(&pixels).ok()? {
        FrameRef::Gpu(tex) => Some(CachedImage {
            texture: tex.id,
            size,
            place,
        }),
        _ => None,
    }
}

/// キーと、その値が頼った ID。
pub(crate) type Keyed = (FrameCacheKey, Vec<String>);

/// キーを作る材料のうち、呼ぶ場所で決まるもの。
pub(crate) struct KeyEnv<'a> {
    pub project: &'a Project,
    pub quality: Quality,
    /// 色の変換とデコーダーの実装を畳んだ名前。画素が実装で変わりうる。
    pub color_config: &'a str,
}

/// 素材の ID を持つパラメータから、その ID。
fn asset_of(params: &Params) -> Option<&AssetId> {
    params
        .0
        .values()
        .find_map(ParamValue::as_asset)
        .filter(|id| !id.as_str().is_empty())
}

/// レイヤーの画像のキー。ノードの種類に渡す直前の `frame`（大きさと領域は倍率を掛けた後）から作る。
/// 決定的でない種類や、素材が引けないものは `None`（覚えない）。
pub(crate) fn kind_key(
    env: &KeyEnv<'_>,
    kind: &dyn NodeKind,
    layer: &Layer,
    frame: &FrameCx<'_>,
) -> Option<Keyed> {
    let traits = kind.traits();
    if !traits.deterministic {
        return None;
    }
    let mut deps = vec![layer.id.as_str().to_owned()];
    let mut content_hash = None;
    if let Some(id) = asset_of(frame.params) {
        content_hash = Some(env.project.asset(id).ok()?.content_hash.clone());
        deps.push(id.as_str().to_owned());
    }
    let source_frame = frame
        .footage
        .as_deref()
        .map(|f| f.info().frame_index_at(frame.source_time));
    // 素材が無い時間で変わる種類は、時刻そのものがキーに要る。素材があれば素材のフレームの番号で足りる。
    let exact_time =
        (traits.time_varying && source_frame.is_none()).then(|| frame.time.to_string());
    let r = frame.region;
    let params_json = serde_json::json!({
        "p": frame.params,
        "canvas": [frame.size.width, frame.size.height],
        "region": [r.x.to_bits(), r.y.to_bits(), r.width.to_bits(), r.height.to_bits()],
        "t": exact_time,
    })
    .to_string();
    let key = FrameCacheKey {
        kind: kind.id().to_owned(),
        kind_version: kind.version(),
        renderer_version: WGPU_RENDERER_VERSION,
        params_json,
        input_keys: Vec::new(),
        content_hash,
        frame_index: source_frame,
        quality: env.quality,
        color_config: env.color_config.to_owned(),
        size_bucket_milli: bucket_milli(frame.scale),
    };
    Some((key, deps))
}

fn bucket_milli(scale: f32) -> u32 {
    (size_bucket(scale) * 1000.0).round() as u32
}

/// 入れ子のコンポジションの画像のキー。中身の全部（入れ子の先も）の指紋を入力のキーに入れる。
pub(crate) struct NestedKeyInput<'a> {
    pub nodes: &'a dyn NodeRegistry,
    pub kind: &'a dyn NodeKind,
    pub layer: &'a Layer,
    pub params: &'a Params,
    pub nested: &'a Composition,
    /// 入れ子の中の時刻。
    pub t: RationalTime,
    pub size: Size,
    pub scale: f32,
}

pub(crate) fn nested_key(env: &KeyEnv<'_>, input: &NestedKeyInput<'_>) -> Option<Keyed> {
    let mut walk = Walk::default();
    walk.visit(env.project, input.nodes, input.nested, input.t, 1)?;
    let mut deps = walk.deps;
    deps.push(input.layer.id.as_str().to_owned());
    let params_json = serde_json::json!({
        "p": input.params,
        "t": input.t.to_string(),
        "size": [input.size.width, input.size.height],
    })
    .to_string();
    let key = FrameCacheKey {
        kind: input.kind.id().to_owned(),
        kind_version: input.kind.version(),
        renderer_version: WGPU_RENDERER_VERSION,
        params_json,
        input_keys: vec![digest_bytes(walk.fingerprint.as_bytes())],
        content_hash: None,
        frame_index: None,
        quality: env.quality,
        color_config: env.color_config.to_owned(),
        size_bucket_milli: bucket_milli(input.scale),
    };
    Some((key, deps))
}

/// 入れ子の中身を辿って、頼った ID と指紋を集める。
#[derive(Default)]
struct Walk {
    deps: Vec<String>,
    fingerprint: String,
    seen: BTreeSet<String>,
}

impl Walk {
    /// 決定的でない種類や、引けない種類と素材があれば `None`（覚えない）。
    fn visit(
        &mut self,
        project: &Project,
        nodes: &dyn NodeRegistry,
        comp: &Composition,
        t: RationalTime,
        depth: u32,
    ) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.deps.push(comp.id.as_str().to_owned());
        self.fingerprint
            .push_str(&serde_json::to_string(comp).ok()?);
        for layer in &comp.layers {
            self.deps.push(layer.id.as_str().to_owned());
            let (kind, params) = evaluate(nodes, layer, t).ok()?;
            if !kind.traits().deterministic {
                return None;
            }
            if let Some(id) = asset_of(&params) {
                let hash = &project.asset(id).ok()?.content_hash;
                self.deps.push(id.as_str().to_owned());
                self.fingerprint.push_str(&format!("|{id}={hash}"));
            }
            if kind.id() == NESTED_KIND {
                self.visit_nested(project, nodes, layer, &params, t, depth)?;
            }
        }
        Some(())
    }

    fn visit_nested(
        &mut self,
        project: &Project,
        nodes: &dyn NodeRegistry,
        layer: &Layer,
        params: &Params,
        t: RationalTime,
        depth: u32,
    ) -> Option<()> {
        let name = params.get(NESTED_PARAM).and_then(|v| v.as_text())?;
        if name.is_empty() || !self.seen.insert(name.to_owned()) {
            return Some(());
        }
        let id: CompositionId = Id::new(name);
        let nested = project.composition(&id).ok()?;
        self.visit(
            project,
            nodes,
            nested,
            source_time(layer, t).ok()?,
            depth + 1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 退避の形は往復する() {
        // GPU が無くても形の検査はできるので、ヘッダーの長さだけ確かめる。
        assert_eq!(HEADER_BYTES, MAGIC.len() + 8 + 16);
        let pixels = RgbaImage::filled(
            Size::new(2, 1),
            mm_types::Rgba {
                r: 0.5,
                g: 0.25,
                b: 0.0,
                a: 1.0,
            },
        );
        let image = CachedImage {
            texture: 0,
            size: Size::new(2, 1),
            place: Rect::new(1.0, 2.0, 3.0, 4.0),
        };
        let bytes = encode(&image, &pixels);
        assert_eq!(bytes.len(), HEADER_BYTES + 2 * 4 * 2);
        assert_eq!(&bytes[..4], MAGIC);
    }
}
