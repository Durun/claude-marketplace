//! 文字の実装。cosmic-text で整形（行組み）し、swash で CPU に描く。
//!
//! `ShapedText` が字体の名前と字形の番号を持つので、描画は行組みをやり直さず字形を引くだけ。
//! 整形器と描画器は別々に作れる。フォントの読み込みは重いので、同じプロセスでは
//! [`TextEngine`] を共有して両方を取り出すと速い。

mod layout;
mod raster;

use std::sync::{Arc, Mutex, MutexGuard};

use cosmic_text::{FontSystem, SwashCache};

use mm_text_api::{ShapedText, TextRasterizer, TextShaper, TextStyle};
use mm_types::{MmError, Result, RgbaImage};

struct Inner {
    fonts: Mutex<(FontSystem, SwashCache)>,
}

fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| MmError::Internal("文字エンジンの鍵が壊れている".into()))
}

/// フォントの読み込みと字形のキャッシュを持つ。整形器と描画器はこれを共有できる。
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

    /// 描画器。
    pub fn rasterizer(&self) -> Arc<GlyphonRasterizer> {
        Arc::new(GlyphonRasterizer {
            inner: self.inner.clone(),
        })
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
        let buffer = layout::layout(&mut fonts.0, text, style);
        Ok(layout::to_shaped(&fonts.0, &buffer, text, style))
    }
}

/// swash による CPU の描画。出力は線形・premultiplied の RGBA。
pub struct GlyphonRasterizer {
    inner: Arc<Inner>,
}

impl GlyphonRasterizer {
    /// 自前のフォントの読み込みで作る。
    pub fn new() -> Self {
        Self {
            inner: TextEngine::new().inner,
        }
    }
}

impl Default for GlyphonRasterizer {
    fn default() -> Self {
        Self::new()
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
        let mut fonts = lock(&self.inner.fonts)?;
        let (fs, cache) = &mut *fonts;
        Ok(raster::draw(fs, cache, shaped, style, scale))
    }
}
