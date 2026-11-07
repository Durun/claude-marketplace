//! 縁取りの描画。縁は字の外側に出て、縁なしの描画は変わらない。
//!
//! 期待画像（`fixtures/golden/cpu/`）の更新:
//! `MM_GOLDEN_UPDATE=1 MM_GOLDEN_BACKEND=cpu cargo test -p mm-text-glyphon --test it stroke::`

use std::path::Path;

use mm_testkit::golden::{Golden, GOLDEN_SIZE};
use mm_text_api::{TextRasterizer, TextShaper, TextStroke, TextStyle};
use mm_text_glyphon::TextEngine;
use mm_types::{Rgba, RgbaImage};

/// 期待画像のフォルダー名。文字は CPU の経路で描き、GPU のバックエンドに依らない。
const BACKEND: &str = "cpu";
const STROKE_PX: f32 = 4.0;

fn style(stroke: Option<TextStroke>) -> TextStyle {
    TextStyle {
        font_family: "Yu Gothic".into(),
        size_px: 44.0,
        stroke,
        ..TextStyle::default()
    }
}

fn black_stroke(width_px: f32) -> Option<TextStroke> {
    Some(TextStroke {
        color: Rgba::BLACK,
        width_px,
    })
}

fn draw(text: &str, st: &TextStyle) -> RgbaImage {
    let engine = TextEngine::new();
    let shaped = engine.shaper().shape(text, st).expect("シェーピング");
    engine
        .rasterizer()
        .rasterize(&shaped, st, 1.0)
        .expect("描く")
}

fn ink(img: &RgbaImage) -> f32 {
    img.data().chunks(4).map(|p| p[3]).sum()
}

#[test]
fn 縁を付けるとインクが増え画像の端で切れない() {
    let plain = draw("Stroke 縁", &style(None));
    let stroked = draw("Stroke 縁", &style(black_stroke(STROKE_PX)));
    assert!(ink(&stroked) > ink(&plain) * 1.3, "縁の分だけ増える");
    assert!(stroked.width() >= plain.width() + 2 * STROKE_PX as u32 - 2);
    let edge_alpha: f32 = (0..stroked.width())
        .map(|x| stroked.pixel(x, 0).a + stroked.pixel(x, stroked.height() - 1).a)
        .sum();
    assert!(edge_alpha < 0.5, "縁が画像の上下の端で切れていない");
}

#[test]
fn 縁の太さが_0_なら縁なしと同じ画像() {
    let plain = draw("Stroke 縁", &style(None));
    let zero = draw("Stroke 縁", &style(black_stroke(0.0)));
    assert_eq!(plain, zero);
}

#[test]
fn 縁の色が出る() {
    let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let st = style(Some(TextStroke {
        color: red,
        width_px: STROKE_PX,
    }));
    let img = draw("I", &st);
    let reddish = (0..img.height())
        .flat_map(|y| (0..img.width()).map(move |x| (x, y)))
        .map(|(x, y)| img.pixel(x, y))
        .filter(|p| p.a > 0.9 && p.r > 0.9 && p.g < 0.05)
        .count();
    assert!(reddish > 20, "縁の色の画素がある: {reddish}");
}

#[test]
fn 縁ありの期待画像() {
    let st = style(black_stroke(STROKE_PX));
    let text = draw("Stroke 縁取り", &st);
    let mut canvas = RgbaImage::filled(GOLDEN_SIZE, Rgba::new(0.05, 0.1, 0.3, 1.0));
    canvas.blit_over(&text, 16, 60, 1.0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden");
    let actual = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden-actual");
    let Some(golden) = Golden::open(&root, BACKEND, &actual) else {
        return;
    };
    if let Err(message) = golden.check("text-stroke", &canvas) {
        panic!("{message}");
    }
}
