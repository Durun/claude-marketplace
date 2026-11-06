//! wgpu の `GpuHelper`。テクスチャは番号で引き、CPU の画像との行き来は f32 と f16 の変換で行う。
//! デコーダーから借用したフレームは `import` で番号を付け、`submit` の前後で待ちと返却をまとめて行う。
//! 使い終えたテクスチャとリードバックのバッファは大きさごとに取り置き、毎フレームの確保を避ける。
//!
//! # 取り置きと提出の順序
//!
//! 呼び手（レンダラー）はパスを 1 つのコマンドに積んでから提出するが、`fill`・`upload`・種類の自前のパスは
//! その場で提出する。後から提出したものほど GPU で後に走るという順序だけが保証される。そこで次を決まりにする。
//!
//! - `release_id` で手放したテクスチャは「提出待ち」に置き、`create` は使い回さない。積んだまま提出していない
//!   コマンドが、まだそのテクスチャを読むかもしれないため。
//! - 呼び手は積んだコマンドを提出した直後に `recycle`（フレームの終わりは `after_submit`。中で `recycle` を呼ぶ）を
//!   呼ぶ。それまでに手放したテクスチャがここで取り置きに入り、次の `create` から使い回せる。以後の書き込みは
//!   提出済みのコマンドより後に走るので、読み終えた後になる。
//! - コマンドを積まずにその場で提出する呼び手は、手放した後に `recycle` を呼んでよい。

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use half::f16;
use half::slice::{HalfBitsSliceExt, HalfFloatSliceExt};
use mm_decode_api::GpuFrameHandle;
use mm_node_api::{FrameRef, GpuHelper, GpuTexture};
use mm_types::{MmError, Result, Rgba, RgbaImage, Size};

use crate::context::{GpuContext, FRAME_FORMAT};
use crate::frame::GpuFrameTexture;
use crate::timer::PassTimer;

/// 1 画素のバイト数（RGBA の f16）。
const BYTES_PER_PIXEL: u32 = 8;
/// 1 画素の値の数（RGBA）。
const CHANNELS: usize = 4;
/// wgpu がリードバックの行に課す境界。
const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
/// 大きさごとに取り置く空きのテクスチャの上限。入れ子の深い合成でも足りる数。
const MAX_FREE_TEXTURES: usize = 8;
/// 提出待ちに置くテクスチャの上限。1 フレームで手放す中間のテクスチャ（入れ子とシーケンスの深さの分）に足りる数。
const MAX_RELEASED_TEXTURES: usize = 64;
/// 取り置くリードバックのバッファの上限。先行させる読み戻し（2〜3 枚）に足りる数。
const MAX_FREE_BUFFERS: usize = 4;

/// wgpu の GPU ヘルパー。具象型は `as_any` で降りて `context()` と `texture()` を使える。
pub struct WgpuGpu {
    ctx: Arc<GpuContext>,
    /// このデバイスの上の全パスが書くタイムスタンプ。
    timer: Arc<PassTimer>,
    next_id: u64,
    textures: HashMap<u64, wgpu::Texture>,
    /// 借用しているフレーム。`after_submit` で破棄する。
    borrowed: Vec<(u64, Arc<dyn mm_decode_api::GpuFrame>)>,
    /// 借用した番号。デコーダーのプールのものなので、解放しても取り置かない。
    imported: HashSet<u64>,
    /// 解放された中間形式のテクスチャ。大きさごとに取り置く。
    free: HashMap<(u32, u32), Vec<wgpu::Texture>>,
    /// 手放したが、読むコマンドがまだ提出されていないかもしれないテクスチャ。`recycle` で `free` に移す。
    released: Vec<wgpu::Texture>,
    /// 読み終えたリードバックのバッファ。バイト数ごとに取り置く。
    free_buffers: HashMap<u64, Vec<wgpu::Buffer>>,
}

/// 提出済みで、まだ CPU に写していないリードバック。`WgpuGpu::finish_readback` で画像にする。
pub struct Readback {
    /// 提出したときのコンテキスト。作り直された後の別のデバイスで待つと終わらないので、照合に使う。
    ctx: Arc<GpuContext>,
    buffer: wgpu::Buffer,
    bytes: u64,
    /// 1 行の使っているバイト数。
    row: u32,
    /// 境界に合わせた 1 行のバイト数。
    padded_row: u32,
    rows: u32,
}

impl WgpuGpu {
    /// 空のテクスチャプール。
    pub fn new(ctx: Arc<GpuContext>) -> Self {
        Self {
            timer: Arc::new(PassTimer::new(&ctx)),
            ctx,
            next_id: 0,
            textures: HashMap::new(),
            borrowed: Vec::new(),
            imported: HashSet::new(),
            free: HashMap::new(),
            released: Vec::new(),
            free_buffers: HashMap::new(),
        }
    }

    /// GPU のコンテキスト。
    pub fn context(&self) -> &Arc<GpuContext> {
        &self.ctx
    }

    /// パスの時間を計るタイマー。自前のパスを持つ呼び手は `pass_writes` を `timestamp_writes` に渡す。
    pub fn timer(&self) -> &Arc<PassTimer> {
        &self.timer
    }

    /// 番号のテクスチャ。
    pub fn texture(&self, id: u64) -> Option<&wgpu::Texture> {
        self.textures.get(&id)
    }

    /// 番号がデコーダーから借用したフレームか。借用は次の `submit` までなので、持ち続けるなら写しが要る。
    pub fn is_borrowed(&self, id: u64) -> bool {
        self.imported.contains(&id)
    }

    /// 持っているテクスチャの数（取り置きは含まない）。漏れの検査に使う。
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// 番号のテクスチャを手放す。中間形式のものは提出待ちに置き（`recycle` で取り置きに入る）、借用したものは返す。
    pub fn release_id(&mut self, id: u64) {
        let Some(texture) = self.textures.remove(&id) else {
            return;
        };
        if self.imported.remove(&id) {
            return;
        }
        // 提出しない呼び手でも溜まり続けないよう、上限を超えた分は取り置かずに捨てる。
        if self.released.len() < MAX_RELEASED_TEXTURES {
            self.released.push(texture);
        }
    }

    /// 積んだコマンドを提出した直後に呼ぶ。それまでに手放したテクスチャを取り置きに入れ、使い回せるようにする。
    pub fn recycle(&mut self) {
        for texture in self.released.drain(..) {
            let free = self
                .free
                .entry((texture.width(), texture.height()))
                .or_default();
            if free.len() < MAX_FREE_TEXTURES {
                free.push(texture);
            }
        }
    }

    /// `submit` の直前。借用したフレームのデコードの完了を待たせる。
    pub fn before_submit(&self) {
        for (_, frame) in &self.borrowed {
            if let Some(wait) =
                GpuFrameTexture::of_frame(frame.as_ref()).and_then(|t| t.wait.as_ref())
            {
                wait(&self.ctx.queue);
            }
        }
    }

    /// `submit` の直後。借用したフレームを返し（デコーダーのプールが空く）、手放したテクスチャを取り置く。
    pub fn after_submit(&mut self) {
        for (id, _) in self.borrowed.drain(..) {
            self.textures.remove(&id);
            self.imported.remove(&id);
        }
        self.recycle();
    }

    fn insert(&mut self, texture: wgpu::Texture) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.textures.insert(id, texture);
        id
    }

    /// 透明の（中身が未定の）テクスチャを作って番号を返す。同じ大きさの取り置きがあればそれを使う。
    pub fn create(&mut self, size: Size) -> Result<u64> {
        if let Some(texture) = self
            .free
            .get_mut(&(size.width, size.height))
            .and_then(Vec::pop)
        {
            return Ok(self.insert(texture));
        }
        let max = self.ctx.device.limits().max_texture_dimension_2d;
        if size.width == 0 || size.height == 0 || size.width > max || size.height > max {
            return Err(MmError::InvalidArgument(format!(
                "テクスチャの大きさ {}x{} は使えない（上限 {max}）",
                size.width, size.height
            )));
        }
        let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mm-gpu frame"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Ok(self.insert(texture))
    }

    fn staging_buffer(&mut self, bytes: u64) -> wgpu::Buffer {
        if let Some(buffer) = self.free_buffers.get_mut(&bytes).and_then(Vec::pop) {
            return buffer;
        }
        self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mm-gpu readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        })
    }

    /// テクスチャ全体をバッファへ写す命令を提出する。待たずに返すので、次のフレームの描画と重ねられる。
    /// `bytes_per_pixel` はテクスチャの形式の 1 画素のバイト数。
    pub fn begin_readback(&mut self, texture: &wgpu::Texture, bytes_per_pixel: u32) -> Readback {
        let (width, height) = (texture.width(), texture.height());
        let row = width * bytes_per_pixel;
        let padded_row = row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let bytes = u64::from(padded_row) * u64::from(height);
        let buffer = self.staging_buffer(bytes);
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.ctx.queue.submit([encoder.finish()]);
        Readback {
            ctx: self.ctx.clone(),
            buffer,
            bytes,
            row,
            padded_row,
            rows: height,
        }
    }

    /// 提出したリードバックの完了を待ち、行の余白を外したバイト列にする。バッファは取り置く。
    pub fn finish_readback(&mut self, readback: Readback) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(readback.row as usize * readback.rows as usize);
        self.finish_readback_rows(readback, |line| out.extend_from_slice(line))?;
        Ok(out)
    }

    /// 提出したリードバックの完了を待ち、余白を外した行を上から順に `visit` に渡す。
    /// 写した先で変換する呼び手が、中間のバイト列を作らずに済む。バッファは取り置く。
    pub fn finish_readback_rows(
        &mut self,
        readback: Readback,
        mut visit: impl FnMut(&[u8]),
    ) -> Result<()> {
        let _span = tracing::debug_span!("render.readback").entered();
        let Readback {
            ctx,
            buffer,
            bytes,
            row,
            padded_row,
            ..
        } = readback;
        if !Arc::ptr_eq(&ctx, &self.ctx) {
            return Err(MmError::Internal(
                "リードバックを頼んだ後にデバイスが作り直された".into(),
            ));
        }
        if let Err(e) = map_blocking(&self.ctx.device, &buffer) {
            // 待ちが失敗するのはデバイスが消えたときなので、持ち主に作り直させる。
            self.ctx.mark_lost();
            return Err(e);
        }
        {
            let data = buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|e| MmError::Internal(format!("リードバックの範囲: {e}")))?;
            for line in data.chunks_exact(padded_row as usize) {
                visit(&line[..row as usize]);
            }
        }
        buffer.unmap();
        let free = self.free_buffers.entry(bytes).or_default();
        if free.len() < MAX_FREE_BUFFERS {
            free.push(buffer);
        }
        Ok(())
    }

    /// 中間形式のテクスチャのリードバックを提出する。形式が合わなければ `Unsupported`。
    pub fn begin_frame_readback(&mut self, id: u64, size: Size) -> Result<Readback> {
        let texture = self
            .textures
            .get(&id)
            .ok_or_else(|| MmError::NotFound(format!("テクスチャ {id}")))?;
        // 行の幅と画素のバイト数を FRAME_FORMAT 前提で計算するので、合わないものは読まない。
        if texture.format() != FRAME_FORMAT
            || texture.width() != size.width
            || texture.height() != size.height
        {
            return Err(MmError::Unsupported(format!(
                "リードバックできるのは {FRAME_FORMAT:?} の {}x{} だけ（テクスチャ {id} は {:?} の {}x{}）",
                size.width,
                size.height,
                texture.format(),
                texture.width(),
                texture.height()
            )));
        }
        let texture = texture.clone();
        Ok(self.begin_readback(&texture, BYTES_PER_PIXEL))
    }

    /// 中間形式のリードバックを f32 の画像にする。
    pub fn finish_frame_readback(&mut self, readback: Readback, size: Size) -> Result<RgbaImage> {
        let row_values = size.width as usize * CHANNELS;
        let mut pixels = vec![0f32; row_values * size.height as usize];
        let mut rows = pixels.chunks_exact_mut(row_values);
        let mut spare = Vec::new();
        self.finish_readback_rows(readback, |line| {
            let Some(dst) = rows.next() else {
                return;
            };
            // 写像した範囲の行は通常 2 バイト境界にあるのでそのまま f16 として読み、無いときだけ写してから読む。
            let words: &[u16] = match bytemuck::try_cast_slice(line) {
                Ok(words) => words,
                Err(_) => {
                    spare.clear();
                    spare.extend(
                        line.chunks_exact(2)
                            .map(|b| u16::from_ne_bytes([b[0], b[1]])),
                    );
                    &spare
                }
            };
            let halves: &[f16] = words.reinterpret_cast();
            halves.convert_to_f32_slice(dst);
        })?;
        RgbaImage::from_raw(size, pixels)
    }

    fn read_back(&mut self, id: u64, size: Size) -> Result<RgbaImage> {
        let readback = self.begin_frame_readback(id, size)?;
        self.finish_frame_readback(readback, size)
    }
}

/// バッファの読み取りを頼み、終わるまで待つ。
pub fn map_blocking(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        // 受け手が先にドロップされていても困らない。
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| MmError::Internal(format!("GPU の待ちに失敗: {e}")))?;
    rx.recv()
        .map_err(|e| MmError::Internal(format!("リードバックの通知が来ない: {e}")))?
        .map_err(|e| MmError::Internal(format!("リードバックに失敗: {e}")))
}

impl GpuHelper for WgpuGpu {
    fn upload(&mut self, image: &RgbaImage) -> Result<FrameRef> {
        let size = image.size();
        let id = self.create(size)?;
        let mut halves = vec![f16::ZERO; image.data().len()];
        halves.convert_from_f32_slice(image.data());
        let texture = &self.textures[&id];
        self.ctx.queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice::<u16, u8>(halves.reinterpret_cast()),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * BYTES_PER_PIXEL),
                rows_per_image: Some(size.height),
            },
            texture.size(),
        );
        Ok(FrameRef::Gpu(GpuTexture { id, size }))
    }

    fn download(&mut self, frame: &FrameRef) -> Result<Arc<RgbaImage>> {
        match frame {
            FrameRef::Transparent { size } => Ok(Arc::new(RgbaImage::transparent(*size))),
            FrameRef::Cpu(img) => Ok(img.clone()),
            FrameRef::Gpu(tex) => Ok(Arc::new(self.read_back(tex.id, tex.size)?)),
        }
    }

    fn allocate(&mut self, size: Size) -> Result<FrameRef> {
        self.fill(size, Rgba::TRANSPARENT)
    }

    fn fill(&mut self, size: Size, color: Rgba) -> Result<FrameRef> {
        let id = self.create(size)?;
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let view = self.textures[&id].create_view(&wgpu::TextureViewDescriptor::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mm-gpu clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(color.r),
                        g: f64::from(color.g),
                        b: f64::from(color.b),
                        a: f64::from(color.a),
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: self.timer.pass_writes(),
            ..Default::default()
        });
        self.ctx.queue.submit([encoder.finish()]);
        Ok(FrameRef::Gpu(GpuTexture { id, size }))
    }

    fn release(&mut self, frame: &FrameRef) {
        if let FrameRef::Gpu(tex) = frame {
            self.release_id(tex.id);
        }
    }

    fn import(&mut self, frame: &GpuFrameHandle) -> Result<FrameRef> {
        let texture = GpuFrameTexture::of(frame).ok_or_else(|| {
            MmError::Unsupported(format!(
                "GPU のフレーム {} は wgpu のテクスチャでない",
                frame.id
            ))
        })?;
        let id = self.insert(texture.texture.clone());
        self.borrowed.push((id, frame.frame.clone()));
        self.imported.insert(id);
        Ok(FrameRef::Gpu(GpuTexture {
            id,
            size: frame.size,
        }))
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
