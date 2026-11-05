//! 文字の見本と契約テスト。見本は 1 文字を 1 つの矩形として組み、矩形を塗る。

use std::sync::Arc;

use mm_text_api::{GlyphBox, ShapedLine, ShapedText, TextRasterizer, TextShaper, TextStyle};
use mm_types::{Rect, Result, RgbaImage, Size};

/// 字の大きさに対する 1 文字の幅の比。
pub const GLYPH_ADVANCE: f32 = 0.6;
/// 空白の文字。
const SPACE: char = ' ';

/// 見本の整形。
#[derive(Debug, Clone, Default)]
pub struct SampleShaper;

impl SampleShaper {
    fn advance(style: &TextStyle) -> f32 {
        style.size_px * GLYPH_ADVANCE
    }

    fn line_height(style: &TextStyle) -> f32 {
        style.size_px * style.line_height
    }
}

impl TextShaper for SampleShaper {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn shape(&self, text: &str, style: &TextStyle) -> Result<ShapedText> {
        if text.is_empty() {
            return Ok(ShapedText::empty());
        }
        let adv = Self::advance(style);
        let lh = Self::line_height(style);
        let max_width = style.max_width.unwrap_or(f32::INFINITY);
        let mut lines: Vec<Vec<GlyphBox>> = vec![Vec::new()];
        let mut x = 0.0;
        for (offset, ch) in text.char_indices() {
            if ch == '\n' || (x + adv > max_width && x > 0.0) {
                lines.push(Vec::new());
                x = 0.0;
                if ch == '\n' {
                    continue;
                }
            }
            if ch == SPACE && x == 0.0 && lines.len() > 1 {
                continue;
            }
            let y = (lines.len() - 1) as f32 * lh;
            lines.last_mut().expect("1 行はある").push(GlyphBox {
                rect: Rect::new(x, y, adv, style.size_px),
                byte_offset: offset,
            });
            x += adv;
        }
        let lines: Vec<ShapedLine> = lines
            .into_iter()
            .enumerate()
            .map(|(i, glyphs)| {
                let width = glyphs.len() as f32 * adv;
                ShapedLine {
                    glyphs,
                    rect: Rect::new(0.0, i as f32 * lh, width, lh),
                }
            })
            .collect();
        let bounds = lines.iter().fold(Rect::EMPTY, |acc, l| acc.union(&l.rect));
        Ok(ShapedText { lines, bounds })
    }
}

/// 見本の描画。グリフの矩形をスタイルの色で塗る。
#[derive(Debug, Clone, Default)]
pub struct SampleRasterizer;

/// `bounds * scale` を切り上げた絵の大きさ。
pub fn raster_size(shaped: &ShapedText, scale: f32) -> Size {
    Size::new(
        (shaped.bounds.width * scale).ceil() as u32,
        (shaped.bounds.height * scale).ceil() as u32,
    )
}

impl TextRasterizer for SampleRasterizer {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn rasterize(&self, shaped: &ShapedText, style: &TextStyle, scale: f32) -> Result<RgbaImage> {
        let mut img = RgbaImage::transparent(raster_size(shaped, scale));
        for line in &shaped.lines {
            for g in &line.glyphs {
                let r = Rect::new(
                    (g.rect.x - shaped.bounds.x) * scale,
                    (g.rect.y - shaped.bounds.y) * scale,
                    g.rect.width * scale,
                    g.rect.height * scale,
                );
                img.fill_rect(r, style.color);
            }
        }
        Ok(img)
    }
}

/// 文字の契約。
pub fn contract(
    make_shaper: impl Fn() -> Arc<dyn TextShaper>,
    make_rasterizer: impl Fn() -> Arc<dyn TextRasterizer>,
) {
    let shaper = make_shaper();
    let raster = make_rasterizer();
    let style = TextStyle::default();

    let a = shaper.shape("Hello, world", &style).expect("整形");
    let b = shaper.shape("Hello, world", &style).expect("整形");
    assert_eq!(a, b, "同じ文字列と字で同じ行組み");
    assert_eq!(a.lines.len(), 1, "折り返し無しは 1 行");

    let empty = shaper.shape("", &style).expect("整形");
    assert!(
        empty.lines.is_empty() && empty.bounds.is_empty(),
        "空文字は空"
    );

    let narrow = TextStyle {
        max_width: Some(style.size_px * 3.0),
        ..style.clone()
    };
    let wrapped = shaper.shape("Supercalifragilistic", &narrow).expect("整形");
    assert!(wrapped.lines.len() > 1, "長い 1 語は折り返す");
    let limit = narrow.max_width.unwrap() + style.size_px;
    assert!(
        wrapped.lines.iter().all(|l| l.rect.width <= limit),
        "どの行も幅に収まる"
    );

    let hard = shaper
        .shape("ﬁ 👨‍👩‍👧 日本語\n改行", &style)
        .expect("合字と絵文字で落ちない");
    assert!(hard.lines.len() >= 2, "改行で行が増える");

    for scale in [1.0, 2.0] {
        let img = raster.rasterize(&a, &style, scale).expect("描ける");
        assert_eq!(
            img.size(),
            raster_size(&a, scale),
            "絵の大きさは bounds * scale"
        );
    }
}
