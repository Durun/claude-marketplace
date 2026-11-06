//! 日本語と英語の混在、折り返し、揃えを実際に描いて確かめる。

use mm_text_api::{TextAlign, TextRasterizer, TextShaper, TextStyle};
use mm_text_glyphon::{CpuRasterizer, TextEngine};
use mm_types::{Rgba, RgbaImage};

/// 線形の値を sRGB の 8 bit に。
fn to_srgb8(v: f32) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let mut tagged = kind.to_vec();
    tagged.extend_from_slice(body);
    out.extend_from_slice(&tagged);
    out.extend_from_slice(&crc32(&tagged).to_be_bytes());
}

/// 圧縮しない zlib の stored ブロックで PNG を作る。依存を増やさないため。
fn encode_png(img: &RgbaImage, background: Rgba) -> Vec<u8> {
    let (w, h) = (img.width(), img.height());
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let p = background.over(img.pixel(x, y));
            raw.extend_from_slice(&[to_srgb8(p.r), to_srgb8(p.g), to_srgb8(p.b)]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn ink(img: &RgbaImage) -> f32 {
    img.data().chunks(4).map(|p| p[3]).sum()
}

fn style(max_width: Option<f32>) -> TextStyle {
    TextStyle {
        font_family: "Yu Gothic".into(),
        size_px: 40.0,
        max_width,
        ..TextStyle::default()
    }
}

#[test]
fn 日本語と英語の混在を描き同じ入力で同じ画像になる() {
    let engine = TextEngine::new();
    let (shaper, raster) = (engine.shaper(), engine.rasterizer());
    let st = style(Some(560.0));
    let text = "動画編集ソフト movie-maker の字幕テスト。Hello, 世界!\nこの行は折り返しを確かめるために、わざと長く書いてあります。";
    let shaped = shaper.shape(text, &st).expect("シェーピング");
    assert!(shaped.lines.len() >= 3, "折り返しで行が増える");
    assert!(shaped.lines.iter().all(|l| l.rect.width <= 560.5));
    let a = raster.rasterize(&shaped, &st, 1.0).expect("描く");
    let b = raster.rasterize(&shaped, &st, 1.0).expect("描く");
    assert_eq!(a, b, "決定的");
    assert!(ink(&a) > 100.0, "字が描かれている");

    let bg = Rgba::new(0.02, 0.02, 0.05, 1.0);
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(dir.join("text_mixed.png"), encode_png(&a, bg)).expect("書ける");

    let centered = TextStyle {
        align: TextAlign::Center,
        ..st.clone()
    };
    let sh = shaper
        .shape("短い\nもう少し長い行です", &centered)
        .expect("シェーピング");
    assert!(
        sh.lines[0].rect.x > sh.lines[1].rect.x,
        "短い行ほど中央に寄る"
    );
    let img = raster.rasterize(&sh, &centered, 2.0).expect("描く");
    assert_eq!(img.width(), (sh.bounds.width * 2.0).ceil() as u32);
    std::fs::write(dir.join("text_center_x2.png"), encode_png(&img, bg)).expect("書ける");
}

#[test]
fn 別に作ったレンダラーでも同じ画像になる() {
    let engine = TextEngine::new();
    let other = CpuRasterizer::new();
    let st = style(None);
    let shaped = engine
        .shaper()
        .shape("abc 日本語", &st)
        .expect("シェーピング");
    let a = engine
        .rasterizer()
        .rasterize(&shaped, &st, 1.0)
        .expect("描く");
    let b = other.rasterize(&shaped, &st, 1.0).expect("描く");
    assert_eq!(
        a, b,
        "フォントとグリフの番号だけで描けるので、同じフォントがあれば同じ画像"
    );
    assert!(ink(&a) > 10.0);
}

/// 画像の範囲を四方に広げて描き直しても、インクの量が変わらないこと（範囲で切れていない）。
fn assert_no_clipping(name: &str, text: &str, st: &TextStyle, scale: f32) {
    let engine = TextEngine::new();
    let (shaper, raster) = (engine.shaper(), engine.rasterizer());
    let shaped = shaper.shape(text, st).expect("シェーピング");
    let mut wide = shaped.clone();
    let pad = 24.0;
    wide.bounds = mm_types::Rect::new(
        shaped.bounds.x - pad,
        shaped.bounds.y - pad,
        shaped.bounds.width + pad * 2.0,
        shaped.bounds.height + pad * 2.0,
    );
    let image = raster.rasterize(&shaped, st, scale).expect("描く");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let bg = Rgba::new(0.02, 0.02, 0.05, 1.0);
    std::fs::write(dir.join(format!("{name}.png")), encode_png(&image, bg)).expect("書ける");
    let tight = ink(&image);
    let loose = ink(&raster.rasterize(&wide, st, scale).expect("描く"));
    assert!(tight > 10.0, "字が描かれている");
    assert!(
        (loose - tight).abs() <= loose * 0.002,
        "{text:?} が範囲で切れている: 範囲内 {tight}、広げた範囲 {loose}"
    );
}

#[test]
fn 行より高いアクセントや絵文字も画像の範囲で切れない() {
    let tight_lines = TextStyle {
        font_family: "Yu Gothic".into(),
        size_px: 48.0,
        line_height: 0.6,
        ..TextStyle::default()
    };
    assert_no_clipping("text_accent_x1", "ÅÉÑ Ǻ ǘ jgyp", &tight_lines, 1.0);
    assert_no_clipping("text_accent_x2", "ÅÉÑ Ǻ ǘ jgyp", &tight_lines, 2.0);
    assert_no_clipping("text_emoji", "😀🎉 絵文字", &tight_lines, 1.0);
    let wide_glyphs = TextStyle {
        font_family: "Segoe UI".into(),
        size_px: 64.0,
        ..TextStyle::default()
    };
    assert_no_clipping("text_overhang", "fjfjf  WAVE ff", &wide_glyphs, 1.0);
}
