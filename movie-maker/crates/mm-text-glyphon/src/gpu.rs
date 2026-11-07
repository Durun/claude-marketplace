//! glyphon（wgpu）による描画。グリフの atlas、頂点、パイプラインは glyphon が持ち、
//! グリフのビットマップは `ShapedText` のフォントとグリフの番号から swash で取って渡す（glyphon の `CustomGlyph`）。
//!
//! 行組みを持たない `ShapedText` からも描けるよう、glyphon の `Buffer` は空のままにする。
//! 出力は線形・premultiplied の `Rgba16Float` をリードバックた RGBA で、CPU のレンダラーと同じ意味。

use std::sync::{Arc, Mutex};

use cosmic_text::{Buffer, FontSystem, Metrics, SwashCache};
use glyphon::{
    Cache, Color, ColorMode, CustomGlyph, RasterizedCustomGlyph, Resolution, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport,
};
use half::f16;
use mm_gpu::{map_blocking, GpuContext, FRAME_FORMAT};
use mm_text_api::{ShapedText, TextRasterizer, TextStyle};
use mm_types::{MmError, Result, Rgba, RgbaImage, Size};

use crate::glyph_ids::{GlyphIds, IdsExhausted, MAX_GLYPH_IDS};
use crate::raster;
use crate::{lock, Inner};

/// リードバックの行は 256 バイトに揃える。
const ROW_ALIGN: u32 = 256;
/// `Rgba16Float` の 1 画素のバイト数。
const BYTES_PER_PIXEL: u32 = 8;

/// glyphon の部品と、グリフの ID の表。
struct GpuState {
    atlas: TextAtlas,
    renderer: TextRenderer,
    viewport: Viewport,
    glyphs: GlyphIds,
}

impl GpuState {
    fn new(ctx: &GpuContext, id_limit: usize) -> Self {
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
            glyphs: GlyphIds::with_limit(id_limit),
        }
    }
}

/// glyphon による描画。出力は線形・premultiplied の RGBA。
///
/// デバイスが消えたら（`GpuContext::is_lost`）、次の描画の前に同じ求めの共有のコンテキストへ持ち替え、
/// atlas とパイプラインを作り直す。出力は CPU の画像なので、レンダラーと別のデバイスになっても困らない。
pub struct GlyphonRasterizer {
    inner: Arc<Inner>,
    /// 描くデバイスと、その上の glyphon の部品。持ち替えで一緒に替わる。
    state: Mutex<(Arc<GpuContext>, GpuState)>,
    /// 1 つの atlas に振るグリフの ID の数の上限。
    id_limit: usize,
}

impl GlyphonRasterizer {
    /// 自前のフォントの読み込みで作る。
    pub fn new(gpu: Arc<GpuContext>) -> Self {
        Self::with_fonts(crate::TextEngine::new().inner, gpu)
    }

    pub(crate) fn with_fonts(inner: Arc<Inner>, gpu: Arc<GpuContext>) -> Self {
        Self::with_id_limit(inner, gpu, MAX_GLYPH_IDS)
    }

    pub(crate) fn with_id_limit(inner: Arc<Inner>, gpu: Arc<GpuContext>, id_limit: usize) -> Self {
        let state = Mutex::new((gpu.clone(), GpuState::new(&gpu, id_limit)));
        Self {
            inner,
            state,
            id_limit,
        }
    }

    /// いま描いているコンテキスト。持ち替えの後は新しいもの。
    pub fn context(&self) -> Result<Arc<GpuContext>> {
        Ok(lock(&self.state)?.0.clone())
    }
}

/// デバイスが消えていたら、同じ求めの共有のコンテキストに持ち替えて部品を作り直す。
fn ensure_device(slot: &mut (Arc<GpuContext>, GpuState), id_limit: usize) -> Result<()> {
    if !slot.0.is_lost() {
        return Ok(());
    }
    let fresh = GpuContext::shared(slot.0.request())?;
    tracing::warn!("文字の描画の GPU を持ち替えた");
    slot.1 = GpuState::new(&fresh, id_limit);
    slot.0 = fresh;
    Ok(())
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

/// グリフを集めて `CustomGlyph` にする。位置は画像の中の整数の左上。
/// ID を使い切ったら `Err`（途中までの ID は呼び出し側が表ごと破棄する）。
fn collect_glyphs(
    fs: &mut FontSystem,
    cache: &mut SwashCache,
    ids: &mut GlyphIds,
    shaped: &ShapedText,
    style: &TextStyle,
    scale: f32,
) -> std::result::Result<Vec<CustomGlyph>, IdsExhausted> {
    let origin = (shaped.bounds.x, shaped.bounds.y);
    let mut glyphs = Vec::new();
    let mut exhausted = false;
    raster::for_each_glyph_image(
        fs,
        cache,
        shaped,
        style,
        scale,
        origin,
        |key, image, l, t| {
            if exhausted {
                return;
            }
            match ids.intern(key, image) {
                Ok(Some((id, w, h))) => glyphs.push(CustomGlyph {
                    id,
                    left: l as f32,
                    top: t as f32,
                    width: f32::from(w),
                    height: f32::from(h),
                    color: None,
                    snap_to_physical_pixel: true,
                    metadata: 0,
                }),
                Ok(None) => {}
                Err(IdsExhausted) => exhausted = true,
            }
        },
    );
    if exhausted {
        Err(IdsExhausted)
    } else {
        Ok(glyphs)
    }
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
        .map_err(|e| MmError::Internal(format!("リードバックの範囲: {e}")))?;
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
    fn target(ctx: &GpuContext, size: Size) -> Result<wgpu::Texture> {
        let limit = ctx.device.limits().max_texture_dimension_2d;
        if size.width > limit || size.height > limit {
            return Err(MmError::Unsupported(format!(
                "文字の画像 {}x{} が GPU のテクスチャの上限 {limit} を超える",
                size.width, size.height
            )));
        }
        Ok(ctx.device.create_texture(&wgpu::TextureDescriptor {
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
        let mut slot = lock(&self.state)?;
        ensure_device(&mut slot, self.id_limit)?;
        let (ctx, state) = &mut *slot;
        let ctx = ctx.clone();
        let texture = Self::target(&ctx, size)?;
        let glyphs = self.collect(&ctx, state, shaped, style, scale)?;
        self.prepare(&ctx, state, &glyphs, style, size)?;
        Self::render(&ctx, state, &texture)?;
        read_back(&ctx, &texture, size)
    }

    /// グリフに ID を振って集める。ID を使い切ったら表と atlas を作り直して最初から振り直し、
    /// それでも収まらない（1 回の描画だけで上限を超える）ときは `Unsupported`。
    fn collect(
        &self,
        ctx: &GpuContext,
        state: &mut GpuState,
        shaped: &ShapedText,
        style: &TextStyle,
        scale: f32,
    ) -> Result<Vec<CustomGlyph>> {
        for attempt in 0..2 {
            if attempt > 0 {
                *state = GpuState::new(ctx, self.id_limit);
            }
            let mut fonts = lock(&self.inner.fonts)?;
            let (fs, cache) = &mut *fonts;
            if let Ok(glyphs) = collect_glyphs(fs, cache, &mut state.glyphs, shaped, style, scale) {
                return Ok(glyphs);
            }
        }
        Err(MmError::Unsupported(format!(
            "1 回の文字の描画のグリフが多すぎる（上限 {}）",
            self.id_limit
        )))
    }

    /// グリフを glyphon の atlas と頂点に載せる。
    fn prepare(
        &self,
        ctx: &GpuContext,
        state: &mut GpuState,
        glyphs: &[CustomGlyph],
        style: &TextStyle,
        size: Size,
    ) -> Result<()> {
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
            custom_glyphs: glyphs,
        };
        let table = &state.glyphs;
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
                    let b = table.bitmap(req.id)?;
                    (b.width == req.width && b.height == req.height).then(|| {
                        RasterizedCustomGlyph {
                            data: b.data.clone(),
                            content_type: b.content,
                        }
                    })
                },
            )
            .map_err(|e| MmError::Internal(format!("glyphon の prepare: {e}")))
    }

    /// 載せたグリフを `texture` に描く。
    fn render(ctx: &GpuContext, state: &mut GpuState, texture: &wgpu::Texture) -> Result<()> {
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
        Ok(())
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
        if raster::visible_stroke(style).is_some() {
            // 縁は被覆率の膨張で描くので CPU の経路に任せる。glyphon の atlas は縁を持たない。
            let mut fonts = lock(&self.inner.fonts)?;
            let (fs, cache) = &mut *fonts;
            return Ok(raster::draw(fs, cache, shaped, style, scale));
        }
        self.draw(shaped, style, scale, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextEngine;
    use mm_gpu::GpuRequest;
    use mm_text_api::TextShaper;

    /// 異なるグリフを 8 つしか振れないレンダラー。
    const SMALL_LIMIT: usize = 8;

    fn gpu() -> Option<Arc<GpuContext>> {
        match GpuContext::shared(&GpuRequest::default()) {
            Ok(ctx) => Some(ctx),
            Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
                panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
            }
            Err(_) => None,
        }
    }

    fn ink(img: &RgbaImage) -> f32 {
        img.data().iter().sum()
    }

    /// 上限に近い atlas に 1 回で上限を超えるグリフを足すと、ID が重ならず作り直して描ける。
    #[test]
    fn 上限を超えたら作り直して描き直す() {
        let Some(gpu) = gpu() else { return };
        let engine = TextEngine::new();
        let shaper = engine.shaper();
        let limited = GlyphonRasterizer::with_id_limit(engine.inner.clone(), gpu, SMALL_LIMIT);
        let reference = engine.rasterizer();
        let style = TextStyle::default();
        for text in ["ABCDE", "FGHIJ", "KLMNOP", "ABCDEFGH"] {
            let shaped = shaper.shape(text, &style).expect("シェーピング");
            let got = limited.rasterize(&shaped, &style, 1.0).expect("描ける");
            let want = reference.rasterize(&shaped, &style, 1.0).expect("CPU");
            assert!(ink(&got) > 0.0, "{text}: 字が描かれる");
            let diff = got.max_diff(&want).expect("同じ大きさ");
            assert!(
                diff < 0.05,
                "{text}: ID が重なると別の字になる（差 {diff}）"
            );
        }
    }

    /// 消えたデバイスの代わりに共有のコンテキストへ持ち替え、同じ字を描く。共有のものを壊さないよう、
    /// 自前のコンテキストに消失のフラグだけを立てる。
    #[test]
    fn デバイスが消えたら持ち替えて描き続ける() {
        let Some(shared) = gpu() else { return };
        let own = GpuContext::with(shared.request()).expect("作れる");
        let engine = TextEngine::new();
        let rasterizer = GlyphonRasterizer::with_fonts(engine.inner.clone(), own.clone());
        let style = TextStyle::default();
        let shaped = engine.shaper().shape("ABC", &style).expect("シェーピング");
        let before = rasterizer.rasterize(&shaped, &style, 1.0).expect("描ける");
        own.mark_lost();
        let after = rasterizer
            .rasterize(&shaped, &style, 1.0)
            .expect("持ち替えて描ける");
        let now = rasterizer.context().unwrap();
        assert!(!Arc::ptr_eq(&now, &own), "新しいコンテキストに替わる");
        assert!(after.max_diff(&before).expect("同じ大きさ") < 1e-3);
    }

    #[test]
    fn 一回の描画だけで上限を超えるなら_unsupported() {
        let Some(gpu) = gpu() else { return };
        let engine = TextEngine::new();
        let limited = GlyphonRasterizer::with_id_limit(engine.inner.clone(), gpu, SMALL_LIMIT);
        let style = TextStyle::default();
        let shaped = engine
            .shaper()
            .shape("ABCDEFGHIJKLMNOP", &style)
            .expect("シェーピング");
        assert!(matches!(
            limited.rasterize(&shaped, &style, 1.0),
            Err(MmError::Unsupported(_))
        ));
    }
}
