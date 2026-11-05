//! 文字の実装。cosmic-text で整形（行組み）し、swash で CPU に描く。
//!
//! `ShapedText` は字形の ID を持たないため、描画側は整形時の文字列と揃えを手元の台帳から引き、
//! 同じ行組みをもう一度作って描く。整形と描画は同じ `TextEngine` から取り出して使う。

mod layout;
mod raster;

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};

use cosmic_text::{FontSystem, SwashCache};

use mm_text_api::{ShapedText, TextRasterizer, TextShaper, TextStyle};
use mm_types::{MmError, Result, RgbaImage};

pub use layout::TextAlign;

/// 台帳に残す整形結果の上限。超えたら全部捨てる（再整形で復元できるため）。
const LEDGER_LIMIT: usize = 4096;

/// 整形で使った文字列と揃え。描画で同じ行組みを作り直すのに要る。
struct Entry {
    text: String,
    align: TextAlign,
}

struct Inner {
    fonts: Mutex<(FontSystem, SwashCache)>,
    ledger: Mutex<HashMap<u64, Entry>>,
}

fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| MmError::Internal("文字エンジンの鍵が壊れている".into()))
}

/// 整形結果と、描画に効くスタイル（色以外）から台帳の鍵を作る。
fn ledger_key(shaped: &ShapedText, style: &TextStyle) -> u64 {
    let mut h = DefaultHasher::new();
    style.font_family.hash(&mut h);
    style.size_px.to_bits().hash(&mut h);
    style.line_height.to_bits().hash(&mut h);
    style.max_width.map(f32::to_bits).hash(&mut h);
    for line in &shaped.lines {
        line.rect.width.to_bits().hash(&mut h);
        for g in &line.glyphs {
            g.byte_offset.hash(&mut h);
            for v in [g.rect.x, g.rect.y, g.rect.width, g.rect.height] {
                v.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

/// フォントの読み込みと字形のキャッシュを持つ。整形器と描画器はこれを共有する。
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
                ledger: Mutex::new(HashMap::new()),
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
    /// 揃えを指定して行に組む。折り返し幅が無いときは、いちばん長い行に揃える。
    pub fn shape_aligned(
        &self,
        text: &str,
        style: &TextStyle,
        align: TextAlign,
    ) -> Result<ShapedText> {
        if text.is_empty() {
            return Ok(ShapedText::empty());
        }
        let shaped = {
            let mut fonts = lock(&self.inner.fonts)?;
            let buffer = layout::layout(&mut fonts.0, text, style, align);
            layout::to_shaped(&buffer, text, style)
        };
        let mut ledger = lock(&self.inner.ledger)?;
        if ledger.len() >= LEDGER_LIMIT {
            ledger.clear();
        }
        ledger.insert(
            ledger_key(&shaped, style),
            Entry {
                text: text.to_owned(),
                align,
            },
        );
        Ok(shaped)
    }
}

impl TextShaper for CosmicShaper {
    fn id(&self) -> &'static str {
        "cosmic"
    }

    fn shape(&self, text: &str, style: &TextStyle) -> Result<ShapedText> {
        self.shape_aligned(text, style, TextAlign::Left)
    }
}

/// swash による CPU の描画。出力は線形・premultiplied の RGBA。
pub struct GlyphonRasterizer {
    inner: Arc<Inner>,
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
        let (text, align) = {
            let ledger = lock(&self.inner.ledger)?;
            let entry = ledger.get(&ledger_key(shaped, style)).ok_or_else(|| {
                MmError::NotFound("同じエンジンで整形した結果でない（台帳に無い）".into())
            })?;
            (entry.text.clone(), entry.align)
        };
        let mut fonts = lock(&self.inner.fonts)?;
        let (fs, cache) = &mut *fonts;
        let buffer = layout::layout(fs, &text, style, align);
        let origin = (shaped.bounds.x, shaped.bounds.y);
        Ok(raster::draw(fs, cache, &buffer, style, scale, origin, size))
    }
}
