//! wgpu の `GpuHelper`。テクスチャは番号で引き、CPU の絵との行き来は f32 と f16 の変換で行う。
//! デコーダーから借りたコマは `import` で番号を付け、`submit` の前後で待ちと返却をまとめて行う。

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use half::f16;
use mm_decode_api::GpuFrameHandle;
use mm_node_api::{FrameRef, GpuHelper, GpuTexture};
use mm_types::{MmError, Result, RgbaImage, Size};

use crate::context::{GpuContext, FRAME_FORMAT};
use crate::frame::GpuFrameTexture;

/// 1 画素のバイト数（RGBA の f16）。
const BYTES_PER_PIXEL: u32 = 8;
/// wgpu が読み戻しの行に課す境界。
const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

/// wgpu の GPU ヘルパー。種類は `as_any` で降りて `context()` と `texture()` を使える。
pub struct WgpuGpu {
    ctx: Arc<GpuContext>,
    next_id: u64,
    textures: HashMap<u64, wgpu::Texture>,
    /// 借りているコマ。`after_submit` で捨てる。
    borrowed: Vec<(u64, Arc<dyn mm_decode_api::GpuFrame>)>,
}

impl WgpuGpu {
    /// 空のテクスチャ置き場。
    pub fn new(ctx: Arc<GpuContext>) -> Self {
        Self {
            ctx,
            next_id: 0,
            textures: HashMap::new(),
            borrowed: Vec::new(),
        }
    }

    /// GPU の文脈。
    pub fn context(&self) -> &Arc<GpuContext> {
        &self.ctx
    }

    /// 番号のテクスチャ。
    pub fn texture(&self, id: u64) -> Option<&wgpu::Texture> {
        self.textures.get(&id)
    }

    /// 持っているテクスチャの数。漏れの検査に使う。
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// 番号のテクスチャを捨てる。
    pub fn release_id(&mut self, id: u64) {
        self.textures.remove(&id);
    }

    /// `submit` の直前。借りたコマの復号の完了を待たせる。
    pub fn before_submit(&self) {
        for (_, frame) in &self.borrowed {
            if let Some(wait) =
                GpuFrameTexture::of_frame(frame.as_ref()).and_then(|t| t.wait.as_ref())
            {
                wait(&self.ctx.queue);
            }
        }
    }

    /// `submit` の直後。借りたコマを返す（デコーダーのプールが空く）。
    pub fn after_submit(&mut self) {
        for (id, _) in self.borrowed.drain(..) {
            self.textures.remove(&id);
        }
    }

    fn insert(&mut self, texture: wgpu::Texture) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.textures.insert(id, texture);
        id
    }

    /// 透明の（中身が未定の）テクスチャを作って番号を返す。
    pub fn create(&mut self, size: Size) -> Result<u64> {
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

    fn read_back(&self, id: u64, size: Size) -> Result<RgbaImage> {
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
                "読み戻せるのは {FRAME_FORMAT:?} の {}x{} だけ（テクスチャ {id} は {:?} の {}x{}）",
                size.width,
                size.height,
                texture.format(),
                texture.width(),
                texture.height()
            )));
        }
        let _span = tracing::debug_span!("render.readback").entered();
        let padded_row = (size.width * BYTES_PER_PIXEL).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let buffer = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mm-gpu readback"),
            size: u64::from(padded_row) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
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
                    rows_per_image: Some(size.height),
                },
            },
            texture.size(),
        );
        self.ctx.queue.submit([encoder.finish()]);
        map_blocking(&self.ctx.device, &buffer)?;
        let data = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| MmError::Internal(format!("読み戻しの範囲: {e}")))?;
        let mut pixels = Vec::with_capacity(size.area() * 4);
        for row in data.chunks_exact(padded_row as usize) {
            let used = &row[..(size.width * BYTES_PER_PIXEL) as usize];
            pixels.extend(
                used.chunks_exact(2)
                    .map(|b| f16::from_le_bytes([b[0], b[1]]).to_f32()),
            );
        }
        drop(data);
        buffer.unmap();
        RgbaImage::from_raw(size, pixels)
    }
}

/// バッファの読み取りを頼み、終わるまで待つ。
pub fn map_blocking(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        // 受け手が先に落ちていても困らない。
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| MmError::Internal(format!("GPU の待ちに失敗: {e}")))?;
    rx.recv()
        .map_err(|e| MmError::Internal(format!("読み戻しの通知が来ない: {e}")))?
        .map_err(|e| MmError::Internal(format!("読み戻しに失敗: {e}")))
}

impl GpuHelper for WgpuGpu {
    fn upload(&mut self, image: &RgbaImage) -> Result<FrameRef> {
        let size = image.size();
        let id = self.create(size)?;
        let texels: Vec<u8> = image
            .data()
            .iter()
            .flat_map(|v| f16::from_f32(*v).to_le_bytes())
            .collect();
        let texture = &self.textures[&id];
        self.ctx.queue.write_texture(
            texture.as_image_copy(),
            &texels,
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
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
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
                "GPU のコマ {} は wgpu のテクスチャでない",
                frame.id
            ))
        })?;
        let id = self.insert(texture.texture.clone());
        self.borrowed.push((id, frame.frame.clone()));
        Ok(FrameRef::Gpu(GpuTexture {
            id,
            size: frame.size,
        }))
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
