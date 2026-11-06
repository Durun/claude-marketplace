//! 文字の実装。cosmic-text で整形（行組み）し、描画は 2 通り持つ。
//!
//! - [`CpuRasterizer`]（`cpu`）: swash で CPU に描く。
//! - [`GlyphonRasterizer`]（`glyphon`）: glyphon（wgpu）のグリフの atlas とパイプラインで描く。
//!
//! `ShapedText` が字体の名前とグリフの番号を持つので、描画は行組みをやり直さずグリフを引くだけ。
//! 整形器とレンダラーは別々に作れる。フォントの読み込みは重いので、同じプロセスでは
//! [`TextEngine`] を共有して両方を取り出すと速い。

mod glyph_ids;
mod gpu;
mod layout;
mod raster;

pub use gpu::GlyphonRasterizer;

use std::sync::{Arc, Mutex, MutexGuard};

use cosmic_text::{FontSystem, SwashCache};
use mm_gpu::GpuContext;

use mm_text_api::{ShapedText, TextRasterizer, TextShaper, TextStyle};
use mm_types::{MmError, Result, RgbaImage};

pub(crate) struct Inner {
    pub(crate) fonts: Mutex<(FontSystem, SwashCache)>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| MmError::Internal("文字エンジンの鍵が壊れている".into()))
}

/// フォントの読み込みとグリフのキャッシュを持つ。整形器とレンダラーはこれを共有できる。
#[derive(Clone)]
pub struct TextEngine {
    inner: Arc<Inner>,
}

impl TextEngine {
    /// OS にあるフォントを読み込んで作る。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                fonts: Mutex::new((FontSystem::new(), SwashCache::new())),
            }),
        }
    }

    /// 整形器。
    pub fn shaper(&self) -> Arc<CosmicShaper> {
        Arc::new(CosmicShaper {
            inner: self.inner.clone(),
        })
    }

    /// レンダラー。
    pub fn rasterizer(&self) -> Arc<CpuRasterizer> {
        Arc::new(CpuRasterizer {
            inner: self.inner.clone(),
        })
    }

    /// glyphon（wgpu）のレンダラー。フォントの読み込みは CPU のレンダラーと共有する。
    pub fn gpu_rasterizer(&self, gpu: Arc<GpuContext>) -> Arc<GlyphonRasterizer> {
        Arc::new(GlyphonRasterizer::with_fonts(self.inner.clone(), gpu))
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// cosmic-text による整形。日本語と英語の混在、折り返し、揃えを扱う。
pub struct CosmicShaper {
    inner: Arc<Inner>,
}

impl CosmicShaper {
    /// 自前のフォントの読み込みで作る。
    pub fn new() -> Self {
        Self {
            inner: TextEngine::new().inner,
        }
    }
}

impl Default for CosmicShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl TextShaper for CosmicShaper {
    fn id(&self) -> &'static str {
        "cosmic"
    }

    fn shape(&self, text: &str, style: &TextStyle) -> Result<ShapedText> {
        if text.is_empty() {
            return Ok(ShapedText::empty());
        }
        let mut fonts = lock(&self.inner.fonts)?;
        let (fs, cache) = &mut *fonts;
        let buffer = layout::layout(fs, text, style);
        let mut shaped = layout::to_shaped(fs, &buffer, text, style);
        if !shaped.bounds.is_empty() {
            // 送り幅と行の高さの矩形に、グリフのインクの張り出しを足す（斜体、アクセント、絵文字）。
            let ink = raster::ink_bounds(fs, cache, &shaped, style);
            shaped.bounds = shaped.bounds.union(&ink);
        }
        Ok(shaped)
    }
}

/// swash による CPU の描画。出力は線形・premultiplied の RGBA。
pub struct CpuRasterizer {
    inner: Arc<Inner>,
}

impl CpuRasterizer {
    /// 自前のフォントの読み込みで作る。
    pub fn new() -> Self {
        Self {
            inner: TextEngine::new().inner,
        }
    }
}

impl Default for CpuRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextRasterizer for CpuRasterizer {
    fn id(&self) -> &'static str {
        "cpu"
    }

    fn rasterize(&self, shaped: &ShapedText, style: &TextStyle, scale: f32) -> Result<RgbaImage> {
        let size = raster::raster_size(shaped, scale);
        if shaped.lines.is_empty() || size.area() == 0 {
            return Ok(RgbaImage::transparent(size));
        }
        let mut fonts = lock(&self.inner.fonts)?;
        let (fs, cache) = &mut *fonts;
        Ok(raster::draw(fs, cache, shaped, style, scale))
    }
}
