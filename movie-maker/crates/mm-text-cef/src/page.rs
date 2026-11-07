//! 行組みの結果を、描画のページ（`assets/text.html`）に渡す値にする。
//! 行の文字はグリフが指す元の文字列の範囲から取り、ルビの注記を親文字とふりがなに分ける。
//! 注記はシェーパーと同じ [`mm_text_api::parse_ruby`] で読む。

use mm_text_api::{parse_ruby, RubySpan, ShapedText, TextStyle};
use mm_types::Rgba;
use serde_json::{json, Value};

/// 8 bit の最大値。CSS の色の成分の上限。
const MAX8: f32 = 255.0;

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
        other => format!("\"{}\", sans-serif", other.replace(['"', '\\'], "")),
    }
}

/// 1 行の区切り。行のグリフが指す元の文字列の範囲に始まりがある区切りを並べ、地の文字は続けてまとめる。
fn line_segments(text: &str, all: &[RubySpan], lo: usize, hi: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut plain = String::new();
    for span in all.iter().filter(|s| (lo..hi).contains(&s.base.start)) {
        match &span.ruby {
            None => plain.push_str(&text[span.base.clone()]),
            Some(ruby) => {
                if !plain.is_empty() {
                    out.push(json!({ "text": std::mem::take(&mut plain) }));
                }
                out.push(json!({ "text": &text[span.base.clone()], "ruby": &text[ruby.clone()] }));
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
    let all = parse_ruby(text);
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

    #[test]
    fn 書体の名前から引用符と逆斜線を外す() {
        assert_eq!(font_family("A\"B\\C"), "\"ABC\", sans-serif");
        assert_eq!(font_family("serif"), "serif");
    }
}
