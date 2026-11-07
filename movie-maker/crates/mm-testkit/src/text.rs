//! 文字のモックと契約テスト。モックは 1 文字を 1 つの矩形として組み、矩形を塗る。

use std::sync::Arc;

use mm_text_api::{
    FaceRef, GlyphBox, ShapedLine, ShapedText, TextAlign, TextRasterizer, TextShaper, TextStyle,
};
use mm_types::{Rect, Result, RgbaImage, Size};

/// 字の大きさに対する 1 文字の幅の比。
pub const GLYPH_ADVANCE: f32 = 0.6;
/// 空白の文字。
const SPACE: char = ' ';

/// モックのシェーピング。
#[derive(Debug, Clone, Default)]
pub struct SampleShaper;

impl SampleShaper {
    fn advance(style: &TextStyle) -> f32 {
        style.size_px * GLYPH_ADVANCE
    }

    fn line_height(style: &TextStyle) -> f32 {
        style.size_px * style.line_height
    }

    /// 文字を矩形として行に並べる（折り返しと改行）。
    fn break_lines(text: &str, style: &TextStyle) -> Vec<Vec<GlyphBox>> {
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
                byte_len: ch.len_utf8(),
                face: 0,
                glyph_id: ch as u32 as u16,
                origin: [x, y + style.size_px],
            });
            x += adv;
        }
        lines
    }

    /// 揃えの分だけ行を右へずらす量。基準の幅は折り返し幅か、いちばん長い行。
    fn shift(align: TextAlign, line_width: f32, base_width: f32) -> f32 {
        match align {
            TextAlign::Left | TextAlign::Justified => 0.0,
            TextAlign::Center => ((base_width - line_width) / 2.0).max(0.0),
            TextAlign::Right => (base_width - line_width).max(0.0),
        }
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
        let raw = Self::break_lines(text, style);
        let widest = raw.iter().map(|l| l.len() as f32 * adv).fold(0.0, f32::max);
        let base = style.max_width.unwrap_or(widest);
        let lines: Vec<ShapedLine> = raw
            .into_iter()
            .enumerate()
            .map(|(i, mut glyphs)| {
                let width = glyphs.len() as f32 * adv;
                let dx = Self::shift(style.align, width, base);
                for g in &mut glyphs {
                    g.rect.x += dx;
                    g.origin[0] += dx;
                }
                ShapedLine {
                    glyphs,
                    rect: Rect::new(dx, i as f32 * lh, width, lh),
                }
            })
            .collect();
        let bounds = lines.iter().fold(Rect::EMPTY, |acc, l| acc.union(&l.rect));
        let mut shaped = ShapedText {
            lines,
            bounds,
            text: text.to_string(),
            ..ShapedText::empty()
        };
        shaped.intern_face(FaceRef {
            post_script_name: crate::SAMPLE_ID.into(),
            weight: 400,
        });
        Ok(shaped)
    }
}

/// モックの描画。グリフの矩形をスタイルの色で塗る。
#[derive(Debug, Clone, Default)]
pub struct SampleRasterizer;

/// `bounds * scale` を切り上げた画像の大きさ。
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

    let text = "Hello, world";
    let a = shaper.shape(text, &style).expect("シェーピング");
    let b = shaper.shape(text, &style).expect("シェーピング");
    assert_eq!(a, b, "同じ文字列と字で同じ行組み");
    assert_eq!(a.lines.len(), 1, "折り返し無しは 1 行");
    check_glyph_refs(&a, text);

    let empty = shaper.shape("", &style).expect("シェーピング");
    assert!(
        empty.lines.is_empty() && empty.bounds.is_empty(),
        "空文字は空"
    );

    let narrow = TextStyle {
        max_width: Some(style.size_px * 3.0),
        ..style.clone()
    };
    let wrapped = shaper
        .shape("Supercalifragilistic", &narrow)
        .expect("シェーピング");
    assert!(wrapped.lines.len() > 1, "長い 1 語は折り返す");
    let limit = narrow.max_width.unwrap() + style.size_px;
    assert!(
        wrapped.lines.iter().all(|l| l.rect.width <= limit),
        "どの行も幅に収まる"
    );

    let hard = shaper
        .shape("ﬁ 👨‍👩‍👧 日本語\n改行", &style)
        .expect("合字と絵文字でクラッシュしない");
    assert!(hard.lines.len() >= 2, "改行で行が増える");

    check_align(shaper.as_ref(), &style);

    let via_json: ShapedText =
        serde_json::from_str(&serde_json::to_string(&a).expect("書ける")).expect("読める");
    for scale in [1.0, 2.0] {
        let img = raster.rasterize(&a, &style, scale).expect("描ける");
        assert_eq!(
            img.size(),
            raster_size(&a, scale),
            "画像の大きさは bounds * scale"
        );
        let again = raster
            .rasterize(&via_json, &style, scale)
            .expect("往復した結果も描ける");
        assert_eq!(again.size(), img.size());
    }
}

/// グリフの参照が `faces` と文字列の中を指す。
fn check_glyph_refs(shaped: &ShapedText, text: &str) {
    for g in shaped.lines.iter().flat_map(|l| &l.glyphs) {
        assert!(
            (g.face as usize) < shaped.faces.len(),
            "face {} は faces の中",
            g.face
        );
        assert!(
            g.byte_offset + g.byte_len <= text.len() && text.is_char_boundary(g.byte_offset),
            "文字の範囲 {}..{} は文字列の中",
            g.byte_offset,
            g.byte_offset + g.byte_len
        );
    }
}

/// 中央と右の揃えでは、短い行が長い行より右から始まる。
fn check_align(shaper: &dyn TextShaper, style: &TextStyle) {
    let text = "Hello, world\nHi";
    for align in [TextAlign::Center, TextAlign::Right] {
        let s = shaper
            .shape(
                text,
                &TextStyle {
                    align,
                    ..style.clone()
                },
            )
            .expect("シェーピング");
        assert_eq!(s.lines.len(), 2);
        assert!(
            s.lines[1].rect.x > s.lines[0].rect.x,
            "{align:?}: 短い行は右から始まる"
        );
    }
}
