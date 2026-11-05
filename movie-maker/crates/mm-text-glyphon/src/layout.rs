//! cosmic-text の行組みを `ShapedText` に写す。整形と描画で同じ行組みを使うため、ここに 1 つだけ置く。

use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap,
};

use mm_text_api::{GlyphBox, ShapedLine, ShapedText, TextStyle};
use mm_types::Rect;

/// 行の揃え。`TextStyle` に揃えの欄が無いため、この実装の側で受ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    /// 左揃え。
    #[default]
    Left,
    /// 中央揃え。
    Center,
    /// 右揃え。
    Right,
    /// 両端揃え（最終行は左）。
    Justified,
}

impl TextAlign {
    fn to_cosmic(self) -> CosmicAlign {
        match self {
            Self::Left => CosmicAlign::Left,
            Self::Center => CosmicAlign::Center,
            Self::Right => CosmicAlign::Right,
            Self::Justified => CosmicAlign::Justified,
        }
    }
}

fn family(name: &str) -> Family<'_> {
    match name {
        "sans-serif" => Family::SansSerif,
        "serif" => Family::Serif,
        "monospace" => Family::Monospace,
        other => Family::Name(other),
    }
}

/// 行高の画素数。0 以下の指定でも cosmic-text が落ちないようにする。
fn line_height_px(style: &TextStyle) -> f32 {
    (style.size_px * style.line_height).max(1.0)
}

fn build_buffer(
    fs: &mut FontSystem,
    text: &str,
    style: &TextStyle,
    align: TextAlign,
    width: Option<f32>,
) -> Buffer {
    let size = style.size_px.max(1.0);
    let mut buffer = Buffer::new(fs, Metrics::new(size, line_height_px(style)));
    buffer.set_wrap(Wrap::WordOrGlyph);
    buffer.set_size(width, None);
    let attrs = Attrs::new().family(family(&style.font_family));
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(align.to_cosmic()));
    buffer.shape_until_scroll(fs, false);
    buffer
}

fn widest_line(buffer: &Buffer) -> f32 {
    buffer.layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
}

/// 行組みを済ませた cosmic-text の Buffer を返す。揃えの基準になる幅が決まらない
/// （折り返し無し）ときは、いちばん長い行の幅を基準にもう一度組む。
pub(crate) fn layout(
    fs: &mut FontSystem,
    text: &str,
    style: &TextStyle,
    align: TextAlign,
) -> Buffer {
    let buffer = build_buffer(fs, text, style, align, style.max_width);
    if style.max_width.is_some() || align == TextAlign::Left {
        return buffer;
    }
    let width = widest_line(&buffer);
    build_buffer(fs, text, style, align, Some(width))
}

/// 文字列の各行の、全体の中の先頭バイト位置。
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    starts
}

/// Buffer の行組みを `ShapedText` にする。
pub(crate) fn to_shaped(buffer: &Buffer, text: &str, style: &TextStyle) -> ShapedText {
    let starts = line_starts(text);
    let lh = line_height_px(style);
    let lines: Vec<ShapedLine> = buffer
        .layout_runs()
        .map(|run| {
            let base = starts.get(run.line_i).copied().unwrap_or(0);
            let glyphs: Vec<GlyphBox> = run
                .glyphs
                .iter()
                .map(|g| GlyphBox {
                    rect: Rect::new(g.x, run.line_top, g.w, lh),
                    byte_offset: base + g.start,
                })
                .collect();
            let left = glyphs
                .iter()
                .map(|g| g.rect.x)
                .fold(f32::INFINITY, f32::min);
            let left = if left.is_finite() { left } else { 0.0 };
            ShapedLine {
                glyphs,
                rect: Rect::new(left, run.line_top, run.line_w, lh),
            }
        })
        .collect();
    let bottom = lines.iter().map(|l| l.rect.bottom()).fold(0.0, f32::max);
    let mut bounds = lines.iter().fold(Rect::EMPTY, |acc, l| acc.union(&l.rect));
    // 空行だけが続く末尾でも、行の高さ分は全体に含める。
    bounds.y = 0.0;
    bounds.height = bottom;
    if bounds.width <= 0.0 {
        return ShapedText {
            lines,
            bounds: Rect::EMPTY,
        };
    }
    ShapedText { lines, bounds }
}
