//! CEF の要らない検査。モックのホスト（`SampleLauncher`）で文字の契約を回し、ページに渡す値を確かめる。

use std::sync::Arc;

use mm_testkit::host::SampleLauncher;
use mm_testkit::text::{contract, SampleShaper};
use mm_text_api::{LineBreak, ShapedText, TextRasterizer, TextShaper, TextStroke, TextStyle};
use mm_text_cef::{page_params, CefRasterizer};
use mm_text_glyphon::TextEngine;
use mm_types::Rgba;

fn sample_rasterizer() -> Arc<CefRasterizer> {
    Arc::new(CefRasterizer::new(
        Arc::new(SampleLauncher),
        "sample".into(),
    ))
}

#[test]
fn モックのホストで文字の契約を満たす() {
    contract(|| Arc::new(SampleShaper), || sample_rasterizer());
    let engine = TextEngine::new();
    contract(|| engine.shaper(), || sample_rasterizer());
}

#[test]
fn 元の文字列が無い行組みの結果は描けない() {
    let mut shaped = SampleShaper.shape("abc", &TextStyle::default()).unwrap();
    shaped.text.clear();
    let err = sample_rasterizer()
        .rasterize(&shaped, &TextStyle::default(), 1.0)
        .unwrap_err();
    assert!(matches!(err, mm_types::MmError::Unsupported(_)), "{err:?}");
}

#[test]
fn ページには行ごとの文字とルビと縁取りを渡す() {
    let style = TextStyle {
        size_px: 40.0,
        max_width: Some(240.0),
        line_break: LineBreak::Phrase,
        stroke: Some(TextStroke {
            color: Rgba::BLACK,
            width_px: 4.0,
        }),
        font_family: "M PLUS Rounded 1c".into(),
        ..TextStyle::default()
    };
    let text = "｜東京《とうきょう》で曲を作っています";
    let shaped: ShapedText = TextEngine::new().shaper().shape(text, &style).unwrap();
    let p = page_params(&shaped, &style, 2.0);
    let lines = p["lines"].as_array().unwrap();
    assert!(lines.len() >= 2, "{p}");
    assert_eq!(lines[0]["segments"][0]["text"], "東京");
    assert_eq!(lines[0]["segments"][0]["ruby"], "とうきょう");
    let all: String = lines
        .iter()
        .flat_map(|l| l["segments"].as_array().unwrap().iter())
        .map(|s| s["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(all, "東京で曲を作っています", "注記を除いた文字が全部並ぶ");
    assert_eq!(p["style"]["stroke"]["width"], 4.0);
    assert_eq!(p["style"]["font"], "\"M PLUS Rounded 1c\", sans-serif");
    assert_eq!(p["scale"], 2.0);
    assert!(
        lines[0]["y"].as_f64().unwrap() > 4.0,
        "ふりがなと縁の分だけ下がる"
    );
}
