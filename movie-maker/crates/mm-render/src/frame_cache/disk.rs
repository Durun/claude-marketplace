//! 追い出した画像のディスクへの退避と、退避したファイルの読み込み。
//!
//! - 退避: テクスチャをバッファへコピーするコマンドを出すところまでを描画のスレッドで行い、コピーし終えるのを待つ・行の余白を
//!   外す・ファイルに書くのは別のスレッドで行う。スクラブで追い出しが続いても描画が止まらない。
//! - 形: 先頭、キーの JSON の長さと中身、画像の大きさ、置き場所、half float の画素。読み込むときにキーの JSON を
//!   照合するので、64 bit の `digest` が衝突しても別の画像を返さない。

use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use half::f16;
use mm_cache_api::CacheStore;
use mm_gpu::{map_blocking, GpuContext, WgpuGpu};
use mm_node_api::{FrameRef, GpuHelper};
use mm_render_api::FrameCacheKey;
use mm_types::{Rect, RgbaImage, Size};

use super::memo::store_key;
use super::CachedImage;

/// 退避したファイルの先頭。形を変えたら変える。
const MAGIC: &[u8; 4] = b"MMF2";
/// 先頭 + キーの JSON の長さ（u32）。
const PREFIX_BYTES: usize = 4 + 4;
/// 画像の大きさ（u32 × 2）+ 置き場所（f32 × 4）。
const META_BYTES: usize = 8 + 16;
/// 画素あたりのバイト数（Rgba16Float）。
const BYTES_PER_PIXEL: u32 = 8;

/// 別のスレッドへ渡す仕事。
enum Job {
    /// コピーのコマンドを出したバッファを待って、ファイルに書く。
    Write(Box<Pending>),
    /// ここまでの仕事が終わったら知らせる。
    Barrier(mpsc::SyncSender<()>),
}

/// コピーのコマンドを出し終えた退避。
struct Pending {
    ctx: Arc<GpuContext>,
    buffer: wgpu::Buffer,
    padded_row: u32,
    key: FrameCacheKey,
    image: CachedImage,
}

/// 退避を受け持つ。書き込みのスレッドは最初の退避で起こし、ドロップで止める。
pub(crate) struct Spiller {
    store: Arc<dyn CacheStore>,
    jobs: Option<mpsc::Sender<Job>>,
    worker: Option<JoinHandle<()>>,
}

impl Spiller {
    pub fn new(store: Arc<dyn CacheStore>) -> Self {
        Self {
            store,
            jobs: None,
            worker: None,
        }
    }

    /// 退避先から引く。キーの JSON が合わないファイルは引かなかったことにする。
    pub fn get(&self, key: &FrameCacheKey, gpu: &mut dyn GpuHelper) -> Option<CachedImage> {
        let bytes = self.store.get(&store_key(key)).ok().flatten()?;
        decode(&bytes, key, gpu)
    }

    /// テクスチャをバッファへコピーするコマンドを出し、待ちと書き込みを別のスレッドに渡す。
    pub fn spill(&mut self, gpu: &WgpuGpu, key: FrameCacheKey, image: CachedImage) {
        let Some(texture) = gpu.texture(image.texture) else {
            return;
        };
        let ctx = gpu.context().clone();
        let (buffer, padded_row) = copy_to_buffer(&ctx, texture, image.size);
        let pending = Pending {
            ctx,
            buffer,
            padded_row,
            key,
            image,
        };
        self.send(Job::Write(Box::new(pending)));
    }

    /// ここまでに頼んだ退避が書き終わるまで待つ。
    pub fn wait(&mut self) {
        if self.jobs.is_none() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.send(Job::Barrier(tx));
        // 書き込みのスレッドが止まっていれば知らせは来ないので、待つのをやめる。
        let _ = rx.recv();
    }

    fn send(&mut self, job: Job) {
        let jobs = self.jobs.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            let store = self.store.clone();
            self.worker = Some(std::thread::spawn(move || write_loop(store.as_ref(), rx)));
            tx
        });
        if jobs.send(job).is_err() {
            tracing::warn!("フレームのキャッシュの書き込みのスレッドが止まっている");
        }
    }
}

impl Drop for Spiller {
    fn drop(&mut self) {
        // 送信側をドロップすると、残りの仕事を終えてスレッドが抜ける。
        self.jobs = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn write_loop(store: &dyn CacheStore, jobs: mpsc::Receiver<Job>) {
    for job in jobs {
        match job {
            Job::Write(pending) => write_one(store, &pending),
            Job::Barrier(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn write_one(store: &dyn CacheStore, pending: &Pending) {
    let _span = tracing::debug_span!("cache.frame_spill").entered();
    if let Err(e) = map_blocking(&pending.ctx.device, &pending.buffer) {
        tracing::warn!(error = %e, "フレームのキャッシュを退避できない");
        return;
    }
    let row = (pending.image.size.width * BYTES_PER_PIXEL) as usize;
    let mut body = Vec::with_capacity(row * pending.image.size.height as usize);
    if let Ok(data) = pending.buffer.slice(..).get_mapped_range() {
        for line in data.chunks_exact(pending.padded_row as usize) {
            body.extend_from_slice(&line[..row]);
        }
    }
    pending.buffer.unmap();
    let bytes = encode(&pending.key, &pending.image, &body);
    match store.put(&store_key(&pending.key), &bytes) {
        Ok(()) => mm_trace::counter!("cache.frame_spilled_bytes", bytes.len()),
        Err(e) => tracing::warn!(error = %e, "フレームのキャッシュを退避できない"),
    }
}

/// テクスチャ全体を、読み取りのできるバッファへコピーするコマンドを提出する。返すのはバッファと境界に合わせた行のバイト数。
fn copy_to_buffer(ctx: &GpuContext, texture: &wgpu::Texture, size: Size) -> (wgpu::Buffer, u32) {
    let row = size.width * BYTES_PER_PIXEL;
    let padded_row =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render.cache_spill"),
        size: u64::from(padded_row) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render.cache_spill"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(size.height),
            },
        },
        texture.size(),
    );
    ctx.queue.submit([encoder.finish()]);
    (buffer, padded_row)
}

fn key_json(key: &FrameCacheKey) -> Vec<u8> {
    serde_json::to_vec(key).unwrap_or_default()
}

/// 退避の形にする。`body` は half float の画素（行の余白なし）。
pub(crate) fn encode(key: &FrameCacheKey, image: &CachedImage, body: &[u8]) -> Vec<u8> {
    let json = key_json(key);
    let mut out = Vec::with_capacity(PREFIX_BYTES + json.len() + META_BYTES + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&image.size.width.to_le_bytes());
    out.extend_from_slice(&image.size.height.to_le_bytes());
    let p = image.place;
    for v in [p.x, p.y, p.width, p.height] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(body);
    out
}

/// 退避の形を読み、キーが合えば大きさ・置き場所・画素を返す。
pub(crate) fn parse(bytes: &[u8], key: &FrameCacheKey) -> Option<(Size, Rect, Vec<f32>)> {
    let word = |at: usize| -> Option<[u8; 4]> { bytes.get(at..at + 4)?.try_into().ok() };
    if bytes.get(..4)? != MAGIC {
        return None;
    }
    let json_len = u32::from_le_bytes(word(4)?) as usize;
    let json = bytes.get(PREFIX_BYTES..PREFIX_BYTES + json_len)?;
    if json != key_json(key).as_slice() {
        mm_trace::counter!("cache.frame_disk_key_mismatch", 1);
        return None;
    }
    let meta = PREFIX_BYTES + json_len;
    let size = Size::new(
        u32::from_le_bytes(word(meta)?),
        u32::from_le_bytes(word(meta + 4)?),
    );
    let f = |i: usize| word(meta + 8 + 4 * i).map(f32::from_le_bytes);
    let place = Rect::new(f(0)?, f(1)?, f(2)?, f(3)?);
    let body = &bytes[meta + META_BYTES..];
    if body.len() != size.area() * BYTES_PER_PIXEL as usize {
        return None;
    }
    let pixels = body
        .chunks_exact(2)
        .map(|c| f16::from_le_bytes([c[0], c[1]]).to_f32())
        .collect();
    Some((size, place, pixels))
}

/// 退避したファイルを GPU に上げ直す。形かキーが合わなければ `None`（引かなかったことにする）。
fn decode(bytes: &[u8], key: &FrameCacheKey, gpu: &mut dyn GpuHelper) -> Option<CachedImage> {
    let (size, place, data) = parse(bytes, key)?;
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

#[cfg(test)]
mod tests {
    use mm_node_api::Quality;

    use super::*;

    fn key(params: &str) -> FrameCacheKey {
        FrameCacheKey {
            kind: "solid".into(),
            kind_version: 1,
            renderer_version: 1,
            params_json: params.into(),
            input_keys: vec![],
            content_hash: None,
            frame_index: None,
            quality: Quality::Full,
            color_config: "lut".into(),
            size_bucket_milli: 1000,
        }
    }

    fn image() -> CachedImage {
        CachedImage {
            texture: 0,
            size: Size::new(2, 1),
            place: Rect::new(1.0, 2.0, 3.0, 4.0),
        }
    }

    fn body() -> Vec<u8> {
        [0.5f32, 0.25, 0.0, 1.0, 1.0, 0.0, 0.5, 1.0]
            .iter()
            .flat_map(|v| f16::from_f32(*v).to_le_bytes())
            .collect()
    }

    #[test]
    fn 退避の形は往復する() {
        let bytes = encode(&key("{}"), &image(), &body());
        let (size, place, pixels) = parse(&bytes, &key("{}")).expect("読める");
        assert_eq!(size, Size::new(2, 1));
        assert_eq!(place, Rect::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(pixels, vec![0.5, 0.25, 0.0, 1.0, 1.0, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn キーの_json_が違うファイルは引かない() {
        let bytes = encode(&key("{\"a\":1}"), &image(), &body());
        assert!(
            parse(&bytes, &key("{\"a\":2}")).is_none(),
            "digest が同じでも中身で照合する"
        );
    }

    #[test]
    fn 壊れたファイルは引かない() {
        let bytes = encode(&key("{}"), &image(), &body());
        assert!(parse(&bytes[..bytes.len() - 1], &key("{}")).is_none());
        assert!(parse(b"MMF1", &key("{}")).is_none());
    }
}
