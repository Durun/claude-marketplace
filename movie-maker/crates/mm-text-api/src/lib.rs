//! 文字のインターフェース。シェーピング（行組み）と描画を分け、シェーピングの結果はキャッシュできる値にする。
//!
//! 大きさは画面上の倍率 `scale` で描き、平行移動は合成の側でかける（ヒンティングで跳ねない）。
//! [`ShapedText`] はグリフ（フォントの名前とグリフ ID）と元の文字の範囲を持つので、レンダラーは行組みを
//! やり直さずに描ける。シェーパーとレンダラーは別々に作ってよく、JSON を往復したシェーピングの結果も描ける。
//!
//! 契約（`mm_testkit::text::contract`）:
//! - 同じ文字列と同じスタイルで同じ行組み（`ShapedText` が一致）。
//! - 空文字は行 0 で、`bounds` が空。
//! - `max_width` より長い 1 語は折り返され、どの行も `max_width` を超えない（1 文字が超える場合を除く）。
//! - 合字と絵文字を含む文字列でクラッシュしない。
//! - どのグリフも `face` が `faces` の中を指し、`byte_offset..byte_offset + byte_len` が文字列の中。
//! - `align` が `Center` / `Right` のとき、短い行は長い行より右から始まる。
//! - `rasterize` の画像の大きさは `bounds * scale` を切り上げた大きさ。
//! - JSON を往復した `ShapedText` も同じ大きさに描ける。
//!
//! ルビは青空文庫の注記で文字列に書く（`｜親文字《ふりがな》`、漢字の並びの直後の `《ふりがな》`）。
//! 区切りは [`parse_ruby`] で読み、シェーパーとレンダラーが同じ区切りになるようにする。注記を読む実装は
//! 親文字だけを組んで親文字の中では折り返さず、ふりがなの分だけ `bounds` を上に広げる。
//! 描く実装はふりがなを親文字の上に描き、描けない実装は親文字だけを描く。

mod ruby;

pub use ruby::{has_ruby, parse_ruby, RubySpan};

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
    /// プロジェクトのデータに書く名前から。知らない名前は左揃え。
    pub fn parse(name: &str) -> Self {
        match name {
            "center" => Self::Center,
            "right" => Self::Right,
            "justified" => Self::Justified,
            _ => Self::Left,
        }
    }
}

/// 縁取り。字の外側に `width_px` の太さで付く。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextStroke {
    pub color: Rgba,
    /// 字の外側に出る縁の太さ（1 倍の画素）。
    pub width_px: f32,
}

/// 行を折り返してよい位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineBreak {
    /// Unicode の行分割の規則（UAX #14）が許すどこでも。日本語は文字の間で折れる。
    #[default]
    Anywhere,
    /// 語と文節の切れ目だけ（日本語は BudouX の分け方）。1 つの文節が幅を超えるときは文字の間で折る。
    /// 実装が持たなければ `Anywhere` でよい。
    Phrase,
}

/// 文字のスタイル。パラメータが文字と色と位置だけの用途（字幕、テロップ）に足りる範囲。
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
    /// 縁取り。シェーパーは縁の分だけ `bounds` を広げる。描けない実装は縁なしで描く。
    #[serde(default)]
    pub stroke: Option<TextStroke>,
    /// 折り返してよい位置。
    #[serde(default)]
    pub line_break: LineBreak,
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
            stroke: None,
            line_break: LineBreak::Anywhere,
        }
    }
}

/// シェーピングに使ったフェイス。レンダラーはこの名前で同じフェイスを引く。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaceRef {
    /// PostScript 名（モックは `sample`）。
    pub post_script_name: String,
    /// 太さ（CSS の 100..=900）。
    pub weight: u16,
}

/// 1 つのグリフの置き場所。
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
    /// フェイスの中のグリフの番号。
    pub glyph_id: u16,
    /// 1 倍のときのグリフの原点（ベースライン上、グリフのオフセット込み）。
    pub origin: [f32; 2],
}

/// 1 行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedLine {
    pub glyphs: Vec<GlyphBox>,
    /// 行の矩形（1 倍）。
    pub rect: Rect,
}

/// シェーピングの結果。描画の入力で、キャッシュのキーに使える。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedText {
    pub lines: Vec<ShapedLine>,
    /// 全体の矩形（1 倍）。
    pub bounds: Rect,
    /// 使ったフェイス。`GlyphBox::face` が指す。
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    /// 組んだ元の文字列。`GlyphBox::byte_offset` はこの中を指す。行ごとの文字を自前の組版で描き直す
    /// 実装（CSS で描く `cef`）が使う。空なら、そうした実装は描けない。
    #[serde(default)]
    pub text: String,
}

impl ShapedText {
    /// 空。
    pub fn empty() -> Self {
        Self {
            lines: Vec::new(),
            bounds: Rect::EMPTY,
            faces: Vec::new(),
            text: String::new(),
        }
    }

    /// フェイスを登録して索引を返す（同じフェイスは 1 つにまとめる）。
    pub fn intern_face(&mut self, face: FaceRef) -> u32 {
        if let Some(i) = self.faces.iter().position(|f| *f == face) {
            return i as u32;
        }
        self.faces.push(face);
        (self.faces.len() - 1) as u32
    }
}

/// シェーピングと行組み。
pub trait TextShaper: Send + Sync {
    /// 実装の名前（`cosmic`、モックは `sample`）。
    fn id(&self) -> &'static str;
    /// 文字列を行に組む。
    fn shape(&self, text: &str, style: &TextStyle) -> Result<ShapedText>;
}

/// 描画。`scale` は画面上の倍率。
pub trait TextRasterizer: Send + Sync {
    /// 実装の名前（`glyphon`、モックは `sample`）。
    fn id(&self) -> &'static str;
    /// シェーピングの結果を `scale` 倍で描く。画像の原点は `shaped.bounds` の左上。
    fn rasterize(&self, shaped: &ShapedText, style: &TextStyle, scale: f32) -> Result<RgbaImage>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn フェイスは_1_つにまとめる() {
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
