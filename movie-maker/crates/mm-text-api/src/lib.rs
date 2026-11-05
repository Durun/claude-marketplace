//! 文字の口。整形（行組み）と描画を分け、整形の結果はキャッシュできる値にする。
//!
//! 大きさは画面上の倍率 `scale` で描き、平行移動は合成の側でかける（ヒンティングで跳ねない）。
//! [`ShapedText`] は字形（字体の名前と字形 ID）と元の文字の範囲を持つので、描画器は行組みを
//! やり直さずに描ける。整形器と描画器は別々に作ってよく、JSON を往復した整形の結果も描ける。
//!
//! 契約（`mm_testkit::text::contract`）:
//! - 同じ文字列と同じスタイルで同じ行組み（`ShapedText` が一致）。
//! - 空文字は行 0 で、`bounds` が空。
//! - `max_width` より長い 1 語は折り返され、どの行も `max_width` を超えない（1 文字が超える場合を除く）。
//! - 合字と絵文字を含む文字列で落ちない。
//! - どの字形も `face` が `faces` の中を指し、`byte_offset..byte_offset + byte_len` が文字列の中。
//! - `align` が `Center` / `Right` のとき、短い行は長い行より右から始まる。
//! - `rasterize` の絵の大きさは `bounds * scale` を切り上げた大きさ。
//! - JSON を往復した `ShapedText` も同じ大きさに描ける。

use serde::{Deserialize, Serialize};

use mm_types::{Rect, Result, Rgba, RgbaImage};

/// 行の揃え。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    /// 左揃え。
    #[default]
    Left,
    /// 中央揃え。
    Center,
    /// 右揃え。
    Right,
    /// 両端揃え（最終行は左）。実装が持たなければ左揃えでよい。
    Justified,
}

impl TextAlign {
    /// 文書に書く名前から。知らない名前は左揃え。
    pub fn parse(name: &str) -> Self {
        match name {
            "center" => Self::Center,
            "right" => Self::Right,
            "justified" => Self::Justified,
            _ => Self::Left,
        }
    }
}

/// 文字のスタイル。パラメータが文字と色と位置だけの用途（字幕、札）に足りる範囲。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_family: String,
    /// 1 倍のときの字の大きさ（画素）。
    pub size_px: f32,
    pub color: Rgba,
    /// 行の折り返しの幅（画素）。`None` は折り返さない。
    pub max_width: Option<f32>,
    /// 行の高さの倍率。1.0 で字の大きさと同じ。
    pub line_height: f32,
    /// 行の揃え。折り返し幅が無いときは、いちばん長い行に揃える。
    #[serde(default)]
    pub align: TextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: "sans-serif".into(),
            size_px: 32.0,
            color: Rgba::WHITE,
            max_width: None,
            line_height: 1.2,
            align: TextAlign::Left,
        }
    }
}

/// 整形に使った字体。描画器はこの名前で同じ字体を引く。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaceRef {
    /// PostScript 名（見本は `sample`）。
    pub post_script_name: String,
    /// 太さ（CSS の 100..=900）。
    pub weight: u16,
}

/// 1 つの字形の置き場所。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlyphBox {
    /// 1 倍のときの矩形。
    pub rect: Rect,
    /// 元の文字の、文字列の中のバイト位置。
    pub byte_offset: usize,
    /// 元の文字のバイト数。
    pub byte_len: usize,
    /// [`ShapedText::faces`] の索引。
    pub face: u32,
    /// 字体の中の字形の番号。
    pub glyph_id: u16,
    /// 1 倍のときの字形の原点（ベースライン上、字形のオフセット込み）。
    pub origin: [f32; 2],
}

/// 1 行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedLine {
    pub glyphs: Vec<GlyphBox>,
    /// 行の矩形（1 倍）。
    pub rect: Rect,
}

/// 整形の結果。描画の入力で、キャッシュのキーに使える。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedText {
    pub lines: Vec<ShapedLine>,
    /// 全体の矩形（1 倍）。
    pub bounds: Rect,
    /// 使った字体。`GlyphBox::face` が指す。
    #[serde(default)]
    pub faces: Vec<FaceRef>,
}

impl ShapedText {
    /// 空。
    pub fn empty() -> Self {
        Self {
            lines: Vec::new(),
            bounds: Rect::EMPTY,
            faces: Vec::new(),
        }
    }

    /// 字体を登録して索引を返す（同じ字体は 1 つにまとめる）。
    pub fn intern_face(&mut self, face: FaceRef) -> u32 {
        if let Some(i) = self.faces.iter().position(|f| *f == face) {
            return i as u32;
        }
        self.faces.push(face);
        (self.faces.len() - 1) as u32
    }
}

/// 整形と行組み。
pub trait TextShaper: Send + Sync {
    /// 実装の名前（`cosmic`、見本は `sample`）。
    fn id(&self) -> &'static str;
    /// 文字列を行に組む。
    fn shape(&self, text: &str, style: &TextStyle) -> Result<ShapedText>;
}

/// 描画。`scale` は画面上の倍率。
pub trait TextRasterizer: Send + Sync {
    /// 実装の名前（`glyphon`、見本は `sample`）。
    fn id(&self) -> &'static str;
    /// 整形の結果を `scale` 倍で描く。絵の原点は `shaped.bounds` の左上。
    fn rasterize(&self, shaped: &ShapedText, style: &TextStyle, scale: f32) -> Result<RgbaImage>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 字体は_1_つにまとめる() {
        let mut s = ShapedText::empty();
        let face = FaceRef {
            post_script_name: "a".into(),
            weight: 400,
        };
        assert_eq!(s.intern_face(face.clone()), 0);
        assert_eq!(s.intern_face(face), 0);
        assert_eq!(s.faces.len(), 1);
    }

    #[test]
    fn 揃えの名前を読む() {
        assert_eq!(TextAlign::parse("center"), TextAlign::Center);
        assert_eq!(TextAlign::parse("?"), TextAlign::Left);
    }
}
