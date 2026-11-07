//! プレートの置き場所。文字の塊の大きさは `TextShaper` で測り、プレートはその周りに余白を足した大きさにする。
//! `text` は塊の左上を `position` に置くので（`mm-export` の字幕の焼き込みと同じ決まり）、文字の位置はプレートの左上に余白を足した所。
//!
//! フォントを読まずに済ませたい所（形式の検査）は [`EstimateShaper`] で見積もる。

use mm_text_api::{ShapedText, TextShaper, TextStyle};
use mm_types::{Rect, Result};

/// 行の高さの倍率。`text` の既定（`TextStyle::default` の 1.2）と同じ。
const LINE_HEIGHT: f32 = 1.2;
/// 半角の字の幅（字の大きさに対する比）。
const HALF_WIDTH_EM: f32 = 0.6;

/// フォントを読まずに、全角を 1 字、半角を 0.6 字として文字の塊の大きさを見積もるシェーパー。
/// 字の形は返さず、`bounds` だけを持つ。
#[derive(Debug, Clone, Copy, Default)]
pub struct EstimateShaper;

impl TextShaper for EstimateShaper {
    fn id(&self) -> &'static str {
        "estimate"
    }

    fn shape(&self, text: &str, style: &TextStyle) -> Result<ShapedText> {
        let size = style.size_px;
        let mut width: f32 = 0.0;
        let mut lines = 0usize;
        for line in text.split('\n') {
            let w = line
                .chars()
                .map(|c| if c.is_ascii() { HALF_WIDTH_EM } else { 1.0 })
                .sum::<f32>()
                * size;
            match style.max_width {
                Some(max) if w > max => {
                    lines += (w / max).ceil() as usize;
                    width = width.max(max);
                }
                _ => {
                    lines += 1;
                    width = width.max(w);
                }
            }
        }
        let mut shaped = ShapedText::empty();
        shaped.bounds = Rect::new(0.0, 0.0, width, lines as f32 * size * LINE_HEIGHT);
        Ok(shaped)
    }
}

/// プレートと文字の置き場所（コンポジションの画素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
    /// プレートの左上と大きさ。
    pub plate: [f64; 4],
    /// 文字の塊の左上。
    pub text_origin: [f64; 2],
}

/// 大きさ `block`（幅、高さ）の文字の塊に余白を足したプレートを、基準点 `anchor` が `position` に来るように置く。
pub(crate) fn place(
    block: [f64; 2],
    padding: [f64; 2],
    position: [f64; 2],
    anchor: [f64; 2],
) -> Placement {
    let plate_w = block[0] + 2.0 * padding[0];
    let plate_h = block[1] + 2.0 * padding[1];
    let x = position[0] - anchor[0] * plate_w;
    let y = position[1] - anchor[1] * plate_h;
    Placement {
        plate: [x, y, plate_w, plate_h],
        text_origin: [x + padding[0], y + padding[1]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size_of(text: &str, max_width: Option<f32>) -> (f32, f32) {
        let style = TextStyle {
            size_px: 10.0,
            max_width,
            ..TextStyle::default()
        };
        let b = EstimateShaper.shape(text, &style).unwrap().bounds;
        (b.width, b.height)
    }

    #[test]
    fn 見積もりは全角と半角を数え_長い行は折り返す() {
        assert_eq!(size_of("あいう", None), (30.0, 12.0));
        assert!((size_of("ab", None).0 - 12.0).abs() < 1e-5);
        assert_eq!(
            size_of("あいうえお", Some(20.0)),
            (20.0, 36.0),
            "50 を 20 で折ると 3 行"
        );
        assert_eq!(size_of("あ\nい", None).1, 24.0);
    }

    #[test]
    fn 基準点を位置に合わせる() {
        let p = place([20.0, 12.0], [4.0, 2.0], [100.0, 50.0], [0.5, 1.0]);
        assert_eq!(p.plate, [86.0, 34.0, 28.0, 16.0]);
        assert_eq!(p.text_origin, [90.0, 36.0]);
    }
}
