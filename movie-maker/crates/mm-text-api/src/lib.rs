//! 文字の口。整形（行組み）と描画を分け、整形の結果はキャッシュできる値にする。
//!
//! 大きさは画面上の倍率 `scale` で描き、平行移動は合成の側でかける（ヒンティングで跳ねない）。
//!
//! 契約（`mm_testkit::text::contract`）:
//! - 同じ文字列と同じスタイルで同じ行組み（`ShapedText` が一致）。
//! - 空文字は行 0 で、`bounds` が空。
//! - `max_width` より長い 1 語は折り返され、どの行も `max_width` を超えない（1 文字が超える場合を除く）。
//! - 合字と絵文字を含む文字列で落ちない。
//! - `rasterize` の絵の大きさは `bounds * scale` を切り上げた大きさ。

use serde::{Deserialize, Serialize};

use mm_types::{Rect, Result, Rgba, RgbaImage};

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
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: "sans-serif".into(),
            size_px: 32.0,
            color: Rgba::WHITE,
            max_width: None,
            line_height: 1.2,
        }
    }
}

/// 1 つのグリフの置き場所。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlyphBox {
    /// 1 倍のときの矩形。
    pub rect: Rect,
    /// 元の文字の、文字列の中のバイト位置。
    pub byte_offset: usize,
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
}

impl ShapedText {
    /// 空。
    pub fn empty() -> Self {
        Self {
            lines: Vec::new(),
            bounds: Rect::EMPTY,
        }
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
