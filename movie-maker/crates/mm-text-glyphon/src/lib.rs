//! 文字の実装。cosmic-text でシェーピング（行組み）し、描画は 2 通り持つ。
//!
//! - [`CpuRasterizer`]（`cpu`）: swash で CPU に描く。
//! - [`GlyphonRasterizer`]（`glyphon`）: glyphon（wgpu）のグリフの atlas とパイプラインで描く。
//!
//! シェーパーはルビの注記を外して親文字だけを組み、`LineBreak::Phrase` なら文節（BudouX）の中で折らない。
//! 縁取りとふりがなの分だけ `bounds` を広げる。描画は縁取りを描く（glyphon も縁があるときは CPU の経路）。
//! ふりがなは描かない（CSS で描く `mm-text-cef` が描く）。
//!
//! `ShapedText` がフォントの名前とグリフの番号を持つので、描画は行組みをやり直さずグリフを引くだけ。
//! シェーパーとレンダラーは別々に作れる。フォントの読み込みは重いので、同じプロセスでは
//! [`TextEngine`] を共有して両方を取り出すと速い。

mod glyph_ids;
mod gpu;
mod layout;
mod markup;
mod raster;
mod stroke;

pub use gpu::GlyphonRasterizer;

use std::sync::{Arc, Mutex, MutexGuard};

use cosmic_text::{FontSystem, SwashCache};
use mm_gpu::GpuContext;

use mm_text_api::{ShapedText, TextRasterizer, TextShaper, TextStyle};
use mm_types::{MmError, Rect, Result, RgbaImage};

pub(crate) struct Inner {
    pub(crate) fonts: Mutex<(FontSystem, SwashCache)>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| MmError::Internal("文字エンジンのロックが壊れている".into()))
}

/// フォントの読み込みとグリフのキャッシュを持つ。シェーパーとレンダラーはこれを共有できる。
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

    /// シェーパー。
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

/// cosmic-text によるシェーピング。日本語と英語の混在、折り返し、揃えを扱う。
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
        let prepared = markup::prepare(text, style.line_break);
        let mut fonts = lock(&self.inner.fonts)?;
        let (fs, cache) = &mut *fonts;
        let buffer = layout::layout(fs, &prepared.text, style);
        let mut shaped = layout::to_shaped(fs, &buffer, &prepared, style);
        shaped.text = text.to_string();
        if !shaped.bounds.is_empty() {
            // 送り幅と行の高さの矩形に、グリフのインクの張り出しを足す（斜体、アクセント、絵文字）。
            let ink = raster::ink_bounds(fs, cache, &shaped, style);
            shaped.bounds = widen(shaped.bounds.union(&ink), style, prepared.has_ruby);
        }
        Ok(shaped)
    }
}

/// ふりがなの大きさの、親文字に対する割合（CSS の `ruby` の既定の 50%）。
const RUBY_SHARE: f32 = 0.5;

/// ふりがなの分だけ上に、縁取りの分だけ四方に広げる。CSS で描く実装がこの中に収まる。
fn widen(bounds: Rect, style: &TextStyle, has_ruby: bool) -> Rect {
    let ruby = if has_ruby {
        style.size_px * RUBY_SHARE
    } else {
        0.0
    };
    let stroke = style.stroke.map_or(0.0, |s| s.width_px.max(0.0));
    Rect::new(
        bounds.x - stroke,
        bounds.y - stroke - ruby,
        bounds.width + 2.0 * stroke,
        bounds.height + 2.0 * stroke + ruby,
    )
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
