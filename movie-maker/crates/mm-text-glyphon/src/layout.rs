//! cosmic-text の行組みを `ShapedText` に変換する。フォントとグリフの番号を結果に残すので、描画は行組みをやり直さない。

use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap,
};

use mm_text_api::{FaceRef, GlyphBox, ShapedLine, ShapedText, TextAlign, TextStyle};
use mm_types::Rect;

use crate::markup::Prepared;

fn to_cosmic(align: TextAlign) -> CosmicAlign {
    match align {
        TextAlign::Left => CosmicAlign::Left,
        TextAlign::Center => CosmicAlign::Center,
        TextAlign::Right => CosmicAlign::Right,
        TextAlign::Justified => CosmicAlign::Justified,
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

/// 行高の画素数。0 以下の指定でも cosmic-text がクラッシュしないようにする。
pub(crate) fn line_height_px(style: &TextStyle) -> f32 {
    (style.size_px * style.line_height).max(1.0)
}

fn build_buffer(fs: &mut FontSystem, text: &str, style: &TextStyle, width: Option<f32>) -> Buffer {
    let size = style.size_px.max(1.0);
    let mut buffer = Buffer::new(fs, Metrics::new(size, line_height_px(style)));
    buffer.set_wrap(Wrap::WordOrGlyph);
    buffer.set_size(width, None);
    let attrs = Attrs::new().family(family(&style.font_family));
    buffer.set_text(
        text,
        &attrs,
        Shaping::Advanced,
        Some(to_cosmic(style.align)),
    );
    buffer.shape_until_scroll(fs, false);
    buffer
}

fn widest_line(buffer: &Buffer) -> f32 {
    buffer.layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
}

/// 行組みを済ませた cosmic-text の Buffer を返す。揃えの基準になる幅が決まらない
/// （折り返し無し）ときは、いちばん長い行の幅を基準にもう一度組む。
pub(crate) fn layout(fs: &mut FontSystem, text: &str, style: &TextStyle) -> Buffer {
    let buffer = build_buffer(fs, text, style, style.max_width);
    if style.max_width.is_some() || style.align == TextAlign::Left {
        return buffer;
    }
    let width = widest_line(&buffer);
    build_buffer(fs, text, style, Some(width))
}

/// 文字列の各行の、全体の中の先頭バイト位置。
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    starts
}

/// fontdb のフェイスを名前と太さで引ける形にする。
fn face_ref(
    fs: &FontSystem,
    id: cosmic_text::fontdb::ID,
    weight: cosmic_text::fontdb::Weight,
) -> FaceRef {
    let name = fs
        .db()
        .face(id)
        .map(|f| f.post_script_name.clone())
        .unwrap_or_default();
    FaceRef {
        post_script_name: name,
        weight: weight.0,
    }
}

/// 組む文字列のバイト範囲を、元の文字列の範囲にする。挟んだ文字（WORD JOINER）だけのグリフなら `None`。
fn source_range(prepared: &Prepared, start: usize, end: usize) -> Option<(usize, usize)> {
    let (at, len) = prepared.first_source(start, end)?;
    let last = end.checked_sub(1).and_then(|e| prepared.source(e));
    let stop = last.map_or(at + len, |(a, l)| a + l).max(at + len);
    Some((at, stop - at))
}

/// Buffer の行組みを `ShapedText` にする。グリフの位置は元の文字列（注記を外す前）を指す。
pub(crate) fn to_shaped(
    fs: &FontSystem,
    buffer: &Buffer,
    prepared: &Prepared,
    style: &TextStyle,
) -> ShapedText {
    let starts = line_starts(&prepared.text);
    let lh = line_height_px(style);
    let mut shaped = ShapedText::empty();
    let mut lines = Vec::new();
    for run in buffer.layout_runs() {
        let base = starts.get(run.line_i).copied().unwrap_or(0);
        let glyphs: Vec<GlyphBox> = run
            .glyphs
            .iter()
            .filter_map(|g| {
                let (byte_offset, byte_len) = source_range(prepared, base + g.start, base + g.end)?;
                Some(GlyphBox {
                    rect: Rect::new(g.x, run.line_top, g.w, lh),
                    byte_offset,
                    byte_len,
                    face: shaped.intern_face(face_ref(fs, g.font_id, g.font_weight)),
                    glyph_id: g.glyph_id,
                    origin: [
                        g.x + g.font_size * g.x_offset,
                        run.line_y + g.y - g.font_size * g.y_offset,
                    ],
                })
            })
            .collect();
        let left = glyphs
            .iter()
            .map(|g| g.rect.x)
            .fold(f32::INFINITY, f32::min);
        let left = if left.is_finite() { left } else { 0.0 };
        lines.push(ShapedLine {
            glyphs,
            rect: Rect::new(left, run.line_top, run.line_w, lh),
        });
    }
    let bottom = lines.iter().map(|l| l.rect.bottom()).fold(0.0, f32::max);
    let mut bounds = lines.iter().fold(Rect::EMPTY, |acc, l| acc.union(&l.rect));
    // 空行だけが続く末尾でも、行の高さ分は全体に含める。
    bounds.y = 0.0;
    bounds.height = bottom;
    shaped.lines = lines;
    shaped.bounds = if bounds.width <= 0.0 {
        Rect::EMPTY
    } else {
        bounds
    };
    shaped
}
