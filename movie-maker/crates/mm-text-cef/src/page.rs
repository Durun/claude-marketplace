//! 行組みの結果を、描画のページ（`assets/text.html`）に渡す値にする。
//! 行の文字はグリフが指す元の文字列の範囲から取り、ルビの注記（`｜親文字《ふりがな》`）を親文字とふりがなに分ける。
//! 注記の読み方は mm-text-glyphon のシェーパーと同じ（`mm-text-api` の契約）。

use mm_text_api::{ShapedText, TextStyle};
use mm_types::Rgba;
use serde_json::{json, Value};

const BASE_MARK: char = '｜';
const RUBY_OPEN: char = '《';
const RUBY_CLOSE: char = '》';
/// 8 bit の最大値。CSS の色の成分の上限。
const MAX8: f32 = 255.0;

/// 元の文字列の 1 区切り。`ruby` があれば `base` の上に描く。
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// 親文字か地の文字の、元の文字列の中のバイト範囲。
    pub base: std::ops::Range<usize>,
    pub ruby: Option<String>,
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' | '〆' | 'ヶ')
}

/// 文字列を 1 文字ずつの地の文字と、親文字の区切りに分ける。注記の記号は区切りに入れない。
pub fn segments(text: &str) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut mark: Option<usize> = None;
    let mut iter = text.char_indices();
    while let Some((at, c)) = iter.next() {
        if c == BASE_MARK {
            mark = Some(out.len());
            continue;
        }
        if c != RUBY_OPEN || !text[at..].contains(RUBY_CLOSE) {
            out.push(Segment {
                base: at..at + c.len_utf8(),
                ruby: None,
            });
            continue;
        }
        let start = mark.take().unwrap_or_else(|| {
            let run = out
                .iter()
                .rev()
                .take_while(|s| text[s.base.clone()].chars().all(is_kanji))
                .count();
            out.len() - run
        });
        let ruby: String = iter
            .by_ref()
            .map(|(_, c)| c)
            .take_while(|c| *c != RUBY_CLOSE)
            .collect();
        if start < out.len() {
            let base = out[start].base.start..out[out.len() - 1].base.end;
            out.truncate(start);
            out.push(Segment {
                base,
                ruby: Some(ruby),
            });
        }
    }
    out
}

/// 線形の premultiplied の色を CSS の `rgb(r g b / a)`（sRGB、straight）にする。
pub fn css_color(c: Rgba) -> String {
    if c.a <= 0.0 {
        return "transparent".into();
    }
    let encode = |v: f32| {
        let v = (v / c.a).clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        s * MAX8
    };
    format!(
        "rgb({:.3} {:.3} {:.3} / {:.4})",
        encode(c.r),
        encode(c.g),
        encode(c.b),
        c.a.min(1.0)
    )
}

/// CSS の `font-family`。総称の名前は引用符を付けない。
fn font_family(name: &str) -> String {
    match name {
        "sans-serif" | "serif" | "monospace" => name.to_string(),
        other => format!("\"{}\", sans-serif", other.replace('"', "")),
    }
}

/// 1 行の区切り。行のグリフが指す元の文字列の範囲に始まりがある区切りを並べ、地の文字は続けてまとめる。
fn line_segments(text: &str, all: &[Segment], lo: usize, hi: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut plain = String::new();
    for seg in all.iter().filter(|s| (lo..hi).contains(&s.base.start)) {
        match &seg.ruby {
            None => plain.push_str(&text[seg.base.clone()]),
            Some(ruby) => {
                if !plain.is_empty() {
                    out.push(json!({ "text": std::mem::take(&mut plain) }));
                }
                out.push(json!({ "text": &text[seg.base.clone()], "ruby": ruby }));
            }
        }
    }
    if !plain.is_empty() {
        out.push(json!({ "text": plain }));
    }
    out
}

/// ページの `mmSetTime(t, params)` に渡す値。座標は `bounds` の左上を原点にした 1 倍の画素。
pub fn page_params(shaped: &ShapedText, style: &TextStyle, scale: f32) -> Value {
    let text = &shaped.text;
    let all = segments(text);
    let (bx, by) = (shaped.bounds.x, shaped.bounds.y);
    let lines: Vec<Value> = shaped
        .lines
        .iter()
        .filter_map(|line| {
            let lo = line.glyphs.iter().map(|g| g.byte_offset).min()?;
            let hi = line
                .glyphs
                .iter()
                .map(|g| g.byte_offset + g.byte_len)
                .max()?;
            Some(json!({
                "x": line.rect.x - bx,
                "y": line.rect.y - by,
                "h": line.rect.height,
                "segments": line_segments(text, &all, lo, hi),
            }))
        })
        .collect();
    let stroke = style
        .stroke
        .filter(|s| s.width_px > 0.0)
        .map(|s| json!({ "width": s.width_px, "color": css_color(s.color) }));
    json!({
        "scale": scale,
        "lines": lines,
        "style": {
            "font": font_family(&style.font_family),
            "size": style.size_px,
            "color": css_color(style.color),
            "stroke": stroke,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(text: &str) -> Vec<(String, Option<String>)> {
        segments(text)
            .into_iter()
            .map(|s| (text[s.base].to_string(), s.ruby))
            .collect()
    }

    #[test]
    fn 印のある親文字と漢字の並びをルビにする() {
        let p = pieces("｜東京《とうきょう》と漢字《かんじ》");
        assert_eq!(p[0], ("東京".into(), Some("とうきょう".into())));
        assert_eq!(p[1], ("と".into(), None));
        assert_eq!(p[2], ("漢字".into(), Some("かんじ".into())));
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn 閉じていない括弧は文字のまま() {
        assert_eq!(pieces("《あ").len(), 2);
    }

    #[test]
    fn 色は_srgb_の_straight_にする() {
        assert_eq!(
            css_color(Rgba::WHITE),
            "rgb(255.000 255.000 255.000 / 1.0000)"
        );
        assert_eq!(
            css_color(Rgba::from_straight(1.0, 0.0, 0.0, 0.5)),
            "rgb(255.000 0.000 0.000 / 0.5000)"
        );
        assert_eq!(css_color(Rgba::TRANSPARENT), "transparent");
    }
}
