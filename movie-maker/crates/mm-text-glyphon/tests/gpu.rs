//! glyphon（GPU）のレンダラーの検査。GPU が無い環境では飛ばし、`MM_REQUIRE_GPU` があれば失敗にする。
//! CPU のレンダラーと同じシェーピングの結果から描いて、画像が許容差の中で一致することを確かめる。

use std::sync::Arc;
use std::time::Instant;

use mm_gpu::GpuContext;
use mm_testkit::text::contract;
use mm_text_api::{ShapedText, TextAlign, TextRasterizer, TextShaper, TextStyle};
use mm_text_glyphon::TextEngine;
use mm_types::{Rgba, RgbaImage};

/// CPU との最大差の許容。色を sRGB の 8 bit に丸めることと f16 の丸めで、1% 未満に収まる。
const TOLERANCE: f32 = 0.01;

fn gpu() -> Option<Arc<GpuContext>> {
    match GpuContext::shared(&mm_gpu::GpuRequest::default()) {
        Ok(ctx) => Some(ctx),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
    }
}

fn to_srgb8(v: f32) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// 背景の上に重ねて sRGB の PNG にする。
fn write_png(path: &std::path::Path, img: &RgbaImage, bg: Rgba) {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.width(), img.height());
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("PNG のヘッダー");
    let mut raw = Vec::with_capacity(img.width() as usize * img.height() as usize * 3);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let p = bg.over(img.pixel(x, y));
            raw.extend_from_slice(&[to_srgb8(p.r), to_srgb8(p.g), to_srgb8(p.b)]);
        }
    }
    writer.write_image_data(&raw).expect("PNG の本体");
    drop(writer);
    std::fs::write(path, out).expect("書ける");
}

fn style(align: TextAlign, max_width: Option<f32>) -> TextStyle {
    TextStyle {
        font_family: "Yu Gothic".into(),
        size_px: 40.0,
        max_width,
        align,
        color: Rgba::new(0.9, 0.8, 0.3, 1.0),
        ..TextStyle::default()
    }
}

#[test]
fn 文字の契約を満たす() {
    let Some(ctx) = gpu() else { return };
    let engine = TextEngine::new();
    contract(|| engine.shaper(), || engine.gpu_rasterizer(ctx.clone()));
}

/// テストケースの名前、文字列、スタイル。
fn samples() -> Vec<(&'static str, &'static str, TextStyle)> {
    let jp =
        "動画編集ソフトの字幕テスト。日本語の折り返しを確かめるために、わざと長く書いてあります。";
    vec![
        ("japanese", jp, style(TextAlign::Left, Some(560.0))),
        (
            "english",
            "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs.",
            style(TextAlign::Left, Some(560.0)),
        ),
        (
            "emoji",
            "😀🎉 絵文字と text 🚀",
            style(TextAlign::Left, None),
        ),
        (
            "align_center",
            "短い\nもう少し長い行です\nLeft or right?",
            style(TextAlign::Center, None),
        ),
        (
            "align_right",
            "短い\nもう少し長い行です\nLeft or right?",
            style(TextAlign::Right, None),
        ),
        (
            "align_left",
            "短い\nもう少し長い行です\nLeft or right?",
            style(TextAlign::Left, None),
        ),
    ]
}

#[test]
fn cpu_と_gpu_の画像が許容差の中で一致しサンプルを書き出す() {
    let Some(ctx) = gpu() else { return };
    let engine = TextEngine::new();
    let (shaper, cpu, glyphon) = (
        engine.shaper(),
        engine.rasterizer(),
        engine.gpu_rasterizer(ctx),
    );
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("text_samples");
    std::fs::create_dir_all(&dir).expect("作れる");
    let bg = Rgba::new(0.02, 0.02, 0.05, 1.0);
    for (name, text, st) in samples() {
        for scale in [1.0_f32, 2.0] {
            let shaped = shaper.shape(text, &st).expect("シェーピング");
            let a = cpu.rasterize(&shaped, &st, scale).expect("CPU");
            let b = glyphon.rasterize(&shaped, &st, scale).expect("GPU");
            assert_eq!(a.size(), b.size(), "{name} x{scale}: 大きさ");
            let diff = a.max_diff(&b).expect("同じ大きさ");
            eprintln!("{name} x{scale}: 最大差 {diff:.5}");
            assert!(diff < TOLERANCE, "{name} x{scale}: 最大差 {diff}");
            write_png(&dir.join(format!("{name}_x{scale}_cpu.png")), &a, bg);
            write_png(&dir.join(format!("{name}_x{scale}_glyphon.png")), &b, bg);
        }
    }
    eprintln!("サンプルの PNG: {}", dir.display());
}

#[test]
fn json_を往復したシェーピングの結果も_gpu_で描ける() {
    let Some(ctx) = gpu() else { return };
    let engine = TextEngine::new();
    let st = style(TextAlign::Left, None);
    let shaped = engine
        .shaper()
        .shape("往復 abc", &st)
        .expect("シェーピング");
    let json = serde_json::to_string(&shaped).expect("JSON にする");
    let back: ShapedText = serde_json::from_str(&json).expect("JSON から戻す");
    let img = engine
        .gpu_rasterizer(ctx)
        .rasterize(&back, &st, 1.0)
        .expect("描く");
    assert!(img.data().chunks(4).map(|p| p[3]).sum::<f32>() > 10.0);
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).expect("NaN は無い"));
    v[v.len() / 2]
}

/// 1080p で字幕を想定した大きさ（54 px、幅 1800）。1 行と 10 行を描く時間を測る。
/// 既定の判断材料なので値は表に出すだけで、閾値では落とさない。
#[test]
fn 時間を測る() {
    let Some(ctx) = gpu() else { return };
    let engine = TextEngine::new();
    let (shaper, cpu, glyphon) = (
        engine.shaper(),
        engine.rasterizer(),
        engine.gpu_rasterizer(ctx),
    );
    let st = TextStyle {
        size_px: 54.0,
        max_width: Some(1800.0),
        ..style(TextAlign::Left, None)
    };
    let line = "動画編集ソフトの字幕テスト。Hello, movie-maker! 日本語と English の混在。";
    let one = line.to_string();
    let ten = (0..10)
        .map(|i| format!("{i}行目 {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    for (label, text) in [("1 行", one), ("10 行", ten)] {
        let shaped = shaper.shape(&text, &st).expect("シェーピング");
        let r: [(&str, &dyn TextRasterizer); 2] = [("cpu", &*cpu), ("glyphon", &*glyphon)];
        for (name, raster) in r {
            let t0 = Instant::now();
            let first = raster.rasterize(&shaped, &st, 1.0).expect("描く");
            let cold = t0.elapsed().as_secs_f64() * 1e3;
            let warm: Vec<f64> = (0..20)
                .map(|_| {
                    let t = Instant::now();
                    raster.rasterize(&shaped, &st, 1.0).expect("描く");
                    t.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            eprintln!(
                "{label} {name}: 画像 {}x{}、初回 {cold:.2} ms、2 回目以降の中央値 {:.2} ms",
                first.width(),
                first.height(),
                median(warm)
            );
        }
    }
}
