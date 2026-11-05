//! glyphon（wgpu）による描画。字形の atlas、頂点、パイプラインは glyphon が持ち、
//! 字形の絵は `ShapedText` の字体と字形の番号から swash で取って渡す（glyphon の `CustomGlyph`）。
//!
//! 行組みを持たない `ShapedText` からも描けるよう、glyphon の `Buffer` は空のままにする。
//! 出力は線形・premultiplied の `Rgba16Float` を読み戻した RGBA で、CPU の描画器と同じ意味。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cosmic_text::{Buffer, CacheKey, FontSystem, Metrics, SwashCache, SwashContent, SwashImage};
use glyphon::{
    Cache, Color, ColorMode, ContentType, CustomGlyph, RasterizedCustomGlyph, Resolution, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};
use half::f16;
use mm_gpu::{map_blocking, GpuContext, FRAME_FORMAT};
use mm_text_api::{ShapedText, TextRasterizer, TextStyle};
use mm_types::{MmError, Result, Rgba, RgbaImage, Size};

use crate::raster;
use crate::{lock, Inner};

/// 字形の ID は `u16`。使い切る前に atlas ごと作り直す。
const MAX_GLYPH_IDS: usize = 60_000;
/// 読み戻しの行は 256 バイトに揃える。
const ROW_ALIGN: u32 = 256;
/// `Rgba16Float` の 1 画素のバイト数。
const BYTES_PER_PIXEL: u32 = 8;

/// 取り出した字形の絵。atlas に載せる元のデータ。
struct GlyphBitmap {
    content: ContentType,
    data: Vec<u8>,
    width: u16,
    height: u16,
}

/// glyphon の部品と、字形の ID の表。
struct GpuState {
    atlas: TextAtlas,
    renderer: TextRenderer,
    viewport: Viewport,
    ids: HashMap<CacheKey, u16>,
    bitmaps: Vec<Option<GlyphBitmap>>,
}

impl GpuState {
    fn new(ctx: &GpuContext) -> Self {
        let cache = Cache::new(&ctx.device);
        // 出力が線形なので、色は sRGB の指定を線形にして混ぜる Accurate。
        let mut atlas = TextAtlas::with_color_mode(
            &ctx.device,
            &ctx.queue,
            &cache,
            FRAME_FORMAT,
            ColorMode::Accurate,
        );
        let renderer = TextRenderer::new(
            &mut atlas,
            &ctx.device,
            wgpu::MultisampleState::default(),
            None,
        );
        let viewport = Viewport::new(&ctx.device, &cache);
        Self {
            atlas,
            renderer,
            viewport,
            ids: HashMap::new(),
            bitmaps: Vec::new(),
        }
    }

    /// 字形の絵に ID を振る。同じ `CacheKey` は同じ ID（atlas の再利用の鍵）。
    fn intern(&mut self, key: CacheKey, image: &SwashImage) -> Option<(u16, u16, u16)> {
        let content = match image.content {
            SwashContent::Mask => ContentType::Mask,
            SwashContent::Color => ContentType::Color,
            SwashContent::SubpixelMask => return None,
        };
        let (w, h) = (image.placement.width as u16, image.placement.height as u16);
        if w == 0 || h == 0 {
            return None;
        }
        let id = *self.ids.entry(key).or_insert_with(|| {
            self.bitmaps.push(Some(GlyphBitmap {
                content,
                data: image.data.clone(),
                width: w,
                height: h,
            }));
            (self.bitmaps.len() - 1) as u16
        });
        Some((id, w, h))
    }
}

/// glyphon による描画。出力は線形・premultiplied の RGBA。
pub struct GlyphonRasterizer {
    inner: Arc<Inner>,
    gpu: Arc<GpuContext>,
    state: Mutex<GpuState>,
}

impl GlyphonRasterizer {
    /// 自前のフォントの読み込みで作る。
    pub fn new(gpu: Arc<GpuContext>) -> Self {
        Self::with_fonts(crate::TextEngine::new().inner, gpu)
    }

    pub(crate) fn with_fonts(inner: Arc<Inner>, gpu: Arc<GpuContext>) -> Self {
        let state = Mutex::new(GpuState::new(&gpu));
        Self { inner, gpu, state }
    }
}

/// 線形の 1 成分を sRGB の 8 bit に。glyphon の色指定は sRGB。
fn to_srgb8(v: f32) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// premultiplied の線形色を、glyphon の色（sRGB の straight）にする。
fn glyphon_color(c: Rgba) -> Color {
    let (r, g, b) = if c.a > 0.0 {
        (c.r / c.a, c.g / c.a, c.b / c.a)
    } else {
        (0.0, 0.0, 0.0)
    };
    Color::rgba(
        to_srgb8(r),
        to_srgb8(g),
        to_srgb8(b),
        (c.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// 字形を集めて `CustomGlyph` にする。位置は絵の中の整数の左上。
fn collect_glyphs(
    fs: &mut FontSystem,
    cache: &mut SwashCache,
    state: &mut GpuState,
    shaped: &ShapedText,
    style: &TextStyle,
    scale: f32,
) -> Vec<CustomGlyph> {
    let origin = (shaped.bounds.x, shaped.bounds.y);
    let mut glyphs = Vec::new();
    raster::for_each_glyph_image(
        fs,
        cache,
        shaped,
        style,
        scale,
        origin,
        |key, image, l, t| {
            if let Some((id, w, h)) = state.intern(key, image) {
                glyphs.push(CustomGlyph {
                    id,
                    left: l as f32,
                    top: t as f32,
                    width: f32::from(w),
                    height: f32::from(h),
                    color: None,
                    snap_to_physical_pixel: true,
                    metadata: 0,
                });
            }
        },
    );
    glyphs
}

fn read_back(ctx: &GpuContext, texture: &wgpu::Texture, size: Size) -> Result<RgbaImage> {
    let padded_row = (size.width * BYTES_PER_PIXEL).div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mm-text readback"),
        size: u64::from(padded_row) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = ctx
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
    ctx.queue.submit([encoder.finish()]);
    map_blocking(&ctx.device, &buffer)?;
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

impl GlyphonRasterizer {
    fn target(&self, size: Size) -> Result<wgpu::Texture> {
        let limit = self.gpu.device.limits().max_texture_dimension_2d;
        if size.width > limit || size.height > limit {
            return Err(MmError::Unsupported(format!(
                "文字の絵 {}x{} が GPU のテクスチャの上限 {limit} を超える",
                size.width, size.height
            )));
        }
        Ok(self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mm-text target"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }))
    }

    fn draw(
        &self,
        shaped: &ShapedText,
        style: &TextStyle,
        scale: f32,
        size: Size,
    ) -> Result<RgbaImage> {
        let mut state = lock(&self.state)?;
        if state.ids.len() > MAX_GLYPH_IDS {
            *state = GpuState::new(&self.gpu);
        }
        let glyphs = {
            let mut fonts = lock(&self.inner.fonts)?;
            let (fs, cache) = &mut *fonts;
            collect_glyphs(fs, cache, &mut state, shaped, style, scale)
        };
        let ctx = &*self.gpu;
        let state = &mut *state;
        state.viewport.update(
            &ctx.queue,
            Resolution {
                width: size.width,
                height: size.height,
            },
        );
        let buffer = Buffer::new_empty(Metrics::new(1.0, 1.0));
        let area = TextArea {
            buffer: &buffer,
            left: 0.0,
            top: 0.0,
            scale: 1.0,
            bounds: TextBounds::default(),
            default_color: glyphon_color(style.color),
            custom_glyphs: &glyphs,
        };
        let bitmaps = &state.bitmaps;
        let mut fonts = lock(&self.inner.fonts)?;
        let (fs, swash) = &mut *fonts;
        state
            .renderer
            .prepare_with_custom(
                &ctx.device,
                &ctx.queue,
                fs,
                &mut state.atlas,
                &state.viewport,
                [area],
                swash,
                |req| {
                    let b = bitmaps.get(usize::from(req.id))?.as_ref()?;
                    (b.width == req.width && b.height == req.height).then(|| {
                        RasterizedCustomGlyph {
                            data: b.data.clone(),
                            content_type: b.content,
                        }
                    })
                },
            )
            .map_err(|e| MmError::Internal(format!("glyphon の prepare: {e}")))?;
        drop(fonts);
        let texture = self.target(size)?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mm-text"),
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
            state
                .renderer
                .render(&state.atlas, &state.viewport, &mut pass)
                .map_err(|e| MmError::Internal(format!("glyphon の render: {e}")))?;
        }
        ctx.queue.submit([encoder.finish()]);
        state.atlas.trim();
        read_back(ctx, &texture, size)
    }
}

impl TextRasterizer for GlyphonRasterizer {
    fn id(&self) -> &'static str {
        "glyphon"
    }

    fn rasterize(&self, shaped: &ShapedText, style: &TextStyle, scale: f32) -> Result<RgbaImage> {
        let size = raster::raster_size(shaped, scale);
        if shaped.lines.is_empty() || size.area() == 0 {
            return Ok(RgbaImage::transparent(size));
        }
        self.draw(shaped, style, scale, size)
    }
}
