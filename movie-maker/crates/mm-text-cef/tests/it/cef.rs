//! 実際の CEF で文字を描く検査。`CEF_PATH` と `MM_CEF_HOST` が無ければスキップする（`MM_REQUIRE_CEF` があれば失敗）。
//! `MM_CEF_OUT` があればそこへ PNG を書く（目で確かめる）。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use mm_host_cef::CefLauncher;
use mm_text_api::{LineBreak, TextAlign, TextRasterizer, TextShaper, TextStroke, TextStyle};
use mm_text_cef::CefRasterizer;
use mm_text_glyphon::TextEngine;
use mm_types::{Rgba, RgbaImage};

fn host() -> Option<PathBuf> {
    let found = std::env::var_os("CEF_PATH").and(std::env::var_os("MM_CEF_HOST"));
    if found.is_none() {
        assert!(
            std::env::var_os("MM_REQUIRE_CEF").is_none(),
            "MM_REQUIRE_CEF があるのに CEF_PATH か MM_CEF_HOST が無い"
        );
        eprintln!("CEF_PATH か MM_CEF_HOST が無いので CEF の検査をスキップする");
    }
    found.map(PathBuf::from)
}

/// 透過が見えるよう、灰色の上に重ねて PNG に書く。
fn save(name: &str, image: &RgbaImage) {
    let Some(dir) = std::env::var_os("MM_CEF_OUT") else {
        return;
    };
    let path = PathBuf::from(dir).join(format!("{name}.png"));
    let mut board = RgbaImage::filled(image.size(), Rgba::new(0.3, 0.3, 0.3, 1.0));
    board.blit_over(image, 0, 0, 1.0);
    let rgb = mm_testkit::golden::to_srgb8(&board);
    let file = std::fs::File::create(&path).expect("PNG を作れる");
    let mut enc = png::Encoder::new(file, image.width(), image.height());
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgb))
        .expect("PNG を書ける");
    eprintln!("{}", path.display());
}

fn telop_style() -> TextStyle {
    TextStyle {
        font_family: "Yu Gothic UI".into(),
        size_px: 64.0,
        color: Rgba::from_straight(1.0, 0.83, 0.23, 1.0),
        max_width: Some(64.0 * 9.0),
        align: TextAlign::Center,
        line_break: LineBreak::Phrase,
        stroke: Some(TextStroke {
            color: Rgba::new(0.004, 0.006, 0.03, 1.0),
            width_px: 6.0,
        }),
        ..TextStyle::default()
    }
}

#[test]
fn 縁取りとルビと文節の折り返しを_css_で描く() {
    let Some(exe) = host() else { return };
    let shaper = TextEngine::new().shaper();
    let raster = CefRasterizer::new(Arc::new(CefLauncher), exe);
    let style = telop_style();
    let text = "｜空間系《くうかんけい》のエフェクトで奥行きを出してみた";
    let shaped = shaper.shape(text, &style).expect("組める");
    let start = Instant::now();
    let image = raster.rasterize(&shaped, &style, 1.0).expect("描ける");
    let first = start.elapsed();
    let start = Instant::now();
    let again = raster.rasterize(&shaped, &style, 1.0).expect("描ける");
    eprintln!(
        "文字 {}x{}: 最初 {:.1} ms（ホストの起動を含む）、2 回目 {:.1} ms",
        image.width(),
        image.height(),
        first.as_secs_f64() * 1000.0,
        start.elapsed().as_secs_f64() * 1000.0
    );
    save("text-cef-telop", &image);
    assert_eq!(image, again, "同じ入力なら同じ画素");
    assert_eq!(image.pixel(0, 0).a, 0.0, "角は透明");
    let dark = image
        .data()
        .chunks(4)
        .filter(|p| p[3] > 0.99 && p[0] < 0.01)
        .count();
    let yellow = image
        .data()
        .chunks(4)
        .filter(|p| p[3] > 0.99 && p[0] > 0.9)
        .count();
    assert!(
        dark > 500 && yellow > 500,
        "縁と字の色がある: 縁 {dark}、字 {yellow}"
    );
}
