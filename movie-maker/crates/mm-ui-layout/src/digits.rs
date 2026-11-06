//! 7 セグメントの数字。サーフェスに文字を描く仕組みを持たないので、目盛りのラベルは矩形の並びで描く。

use crate::geom::{Color, Quad, Rect};

/// 文字の幅と高さの比。
const ASPECT: f32 = 0.55;
/// 文字どうしの間隔（高さに対する比）。
const GAP: f32 = 0.25;
/// セグメントの太さ（高さに対する比）。
const THICK: f32 = 0.14;

/// セグメントの並び: 上、右上、右下、下、左下、左上、中。
const DIGITS: [u8; 10] = [
    0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
    0b1111111, 0b1101111,
];

/// 文字列の幅（px）。対応する文字は 0-9、`:`、`.`、`-`、空白。
pub fn text_width(text: &str, height: f32) -> f32 {
    text.chars().map(|c| advance(c, height)).sum()
}

fn advance(c: char, height: f32) -> f32 {
    match c {
        ':' | '.' => height * (THICK * 2.0 + GAP),
        _ => height * (ASPECT + GAP),
    }
}

/// `text` を (x, y) を左上にして高さ `height` で描く矩形。
pub fn text_quads(text: &str, x: f32, y: f32, height: f32, color: Color) -> Vec<Quad> {
    let mut out = Vec::new();
    let mut cx = x;
    for c in text.chars() {
        glyph(c, cx, y, height, color, &mut out);
        cx += advance(c, height);
    }
    out
}

fn glyph(c: char, x: f32, y: f32, h: f32, color: Color, out: &mut Vec<Quad>) {
    let t = (h * THICK).max(1.0);
    let w = h * ASPECT;
    let mid = y + (h - t) / 2.0;
    let mut seg = |rect| out.push(Quad::new(rect, color));
    match c {
        '0'..='9' => {
            let mask = DIGITS[c as usize - '0' as usize];
            let (top, bottom) = (y, y + h - t);
            let (upper, lower) = (y, y + h / 2.0);
            let half = h / 2.0;
            let parts = [
                Rect::new(x, top, w, t),
                Rect::new(x + w - t, upper, t, half),
                Rect::new(x + w - t, lower, t, half),
                Rect::new(x, bottom, w, t),
                Rect::new(x, lower, t, half),
                Rect::new(x, upper, t, half),
                Rect::new(x, mid, w, t),
            ];
            for (i, rect) in parts.into_iter().enumerate() {
                if mask & (1 << i) != 0 {
                    seg(rect);
                }
            }
        }
        ':' => {
            seg(Rect::new(x, y + h * 0.28, t, t));
            seg(Rect::new(x, y + h * 0.66, t, t));
        }
        '.' => seg(Rect::new(x, y + h - t, t, t)),
        '-' => seg(Rect::new(x, mid, w, t)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Color = Color([1.0; 4]);

    #[test]
    fn 八は七つのセグメントで_一は二つ() {
        assert_eq!(text_quads("8", 0.0, 0.0, 20.0, C).len(), 7);
        assert_eq!(text_quads("1", 0.0, 0.0, 20.0, C).len(), 2);
    }

    #[test]
    fn 描いた範囲は幅と高さの内側に収まる() {
        let h = 16.0;
        let text = "12:30.5";
        let w = text_width(text, h);
        for q in text_quads(text, 10.0, 4.0, h, C) {
            assert!(
                q.rect.x >= 10.0 && q.rect.right() <= 10.0 + w + 0.01,
                "{q:?}"
            );
            assert!(
                q.rect.y >= 4.0 && q.rect.bottom() <= 4.0 + h + 0.01,
                "{q:?}"
            );
        }
    }

    #[test]
    fn 対応しない文字は幅だけ進めて何も描かない() {
        assert!(text_quads("  ", 0.0, 0.0, 10.0, C).is_empty());
        assert!(text_width("  ", 10.0) > 0.0);
    }
}
