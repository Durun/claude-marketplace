//! `ShapedText` の字形を、字体の名前と字形の番号で引いて CPU で RGBA に描く。

use std::collections::HashMap;

use cosmic_text::fontdb::{Weight, ID};
use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem, SwashCache, SwashContent};

use mm_text_api::{FaceRef, ShapedText, TextStyle};
use mm_types::{Rect, Rgba, RgbaImage, Size};

/// インクの矩形に足す余白（1 倍の画素）。倍率ごとのヒンティングのずれで端が切れないようにする。
const INK_MARGIN: f32 = 1.0;

/// 絵の大きさは `bounds * scale` を切り上げた大きさ（口の契約）。
pub(crate) fn raster_size(shaped: &ShapedText, scale: f32) -> Size {
    Size::new(
        (shaped.bounds.width * scale).ceil() as u32,
        (shaped.bounds.height * scale).ceil() as u32,
    )
}

/// sRGB の 8 bit を線形にする。色つきの字形（絵文字）は sRGB で来るため。
fn srgb_to_linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// 字形の 1 画素の色。Mask は文字色にカバレッジを掛け、Color は字形の色そのまま。
fn pixel_color(content: SwashContent, data: &[u8], index: usize, color: Rgba) -> Option<Rgba> {
    match content {
        SwashContent::Mask => {
            let coverage = f32::from(*data.get(index)?) / 255.0;
            Some(Rgba::new(
                color.r * coverage,
                color.g * coverage,
                color.b * coverage,
                color.a * coverage,
            ))
        }
        SwashContent::Color => {
            let px = data.get(index * 4..index * 4 + 4)?;
            let a = f32::from(px[3]) / 255.0;
            Some(Rgba::from_straight(
                srgb_to_linear(px[0]),
                srgb_to_linear(px[1]),
                srgb_to_linear(px[2]),
                a,
            ))
        }
        SwashContent::SubpixelMask => None,
    }
}

/// `faces` の各字体を fontdb の ID に引く。無い字体は `None`（その字形は描かない）。
fn resolve_faces(fs: &FontSystem, faces: &[FaceRef]) -> Vec<Option<(ID, Weight)>> {
    let mut by_name: HashMap<&str, ID> = HashMap::new();
    for face in fs.db().faces() {
        by_name
            .entry(face.post_script_name.as_str())
            .or_insert(face.id);
    }
    faces
        .iter()
        .map(|f| {
            by_name
                .get(f.post_script_name.as_str())
                .map(|id| (*id, Weight(f.weight)))
        })
        .collect()
}

/// 字形の絵を `img` に重ねる。`left`/`top` は絵の中の左上。
fn blend_glyph(
    img: &mut RgbaImage,
    image: &cosmic_text::SwashImage,
    left: i32,
    top: i32,
    color: Rgba,
) {
    let (w, h) = (
        image.placement.width as usize,
        image.placement.height as usize,
    );
    for row in 0..h {
        for col in 0..w {
            let Some(src) = pixel_color(image.content, &image.data, row * w + col, color) else {
                continue;
            };
            let (x, y) = (left + col as i32, top + row as i32);
            if x < 0 || y < 0 || x as u32 >= img.width() || y as u32 >= img.height() {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            let under = img.pixel(x, y);
            img.set_pixel(x, y, under.over(src));
        }
    }
}

/// `shaped` の字形を `scale` 倍で描く。絵の原点は `shaped.bounds` の左上。
pub(crate) fn draw(
    fs: &mut FontSystem,
    cache: &mut SwashCache,
    shaped: &ShapedText,
    style: &TextStyle,
    scale: f32,
) -> RgbaImage {
    let size = raster_size(shaped, scale);
    let mut img = RgbaImage::transparent(size);
    let origin = (shaped.bounds.x, shaped.bounds.y);
    for_each_glyph_image(
        fs,
        cache,
        shaped,
        style,
        scale,
        origin,
        |_key, image, left, top| {
            blend_glyph(&mut img, image, left, top, style.color);
        },
    );
    img
}

/// 字形のインクが 1 倍で占める矩形の和。行の送り幅や高さを越える部分（斜体の張り出し、
/// 行より高いアクセントや絵文字）を絵の範囲に含めるために使う。
pub(crate) fn ink_bounds(
    fs: &mut FontSystem,
    cache: &mut SwashCache,
    shaped: &ShapedText,
    style: &TextStyle,
) -> Rect {
    let mut bounds = Rect::EMPTY;
    for_each_glyph_image(
        fs,
        cache,
        shaped,
        style,
        1.0,
        (0.0, 0.0),
        |_key, image, left, top| {
            let ink = Rect::new(
                left as f32 - INK_MARGIN,
                top as f32 - INK_MARGIN,
                image.placement.width as f32 + INK_MARGIN * 2.0,
                image.placement.height as f32 + INK_MARGIN * 2.0,
            );
            bounds = bounds.union(&ink);
        },
    );
    bounds
}

/// 各字形の絵を引き、絵の左上の位置（`origin` を原点とする `scale` 倍の画素）と一緒に `f` に渡す。
pub(crate) fn for_each_glyph_image(
    fs: &mut FontSystem,
    cache: &mut SwashCache,
    shaped: &ShapedText,
    style: &TextStyle,
    scale: f32,
    origin: (f32, f32),
    mut f: impl FnMut(CacheKey, &cosmic_text::SwashImage, i32, i32),
) {
    let faces = resolve_faces(fs, &shaped.faces);
    for glyph in shaped.lines.iter().flat_map(|l| &l.glyphs) {
        let Some(Some((font_id, weight))) = faces.get(glyph.face as usize) else {
            continue;
        };
        let px = (glyph.origin[0] - origin.0) * scale;
        // Y はヒンティングのため整数に切る（cosmic-text の physical と同じ）。
        let py = ((glyph.origin[1] - origin.1) * scale).trunc();
        let (key, x, y) = CacheKey::new(
            *font_id,
            glyph.glyph_id,
            style.size_px.max(1.0) * scale,
            (px, py),
            *weight,
            CacheKeyFlags::empty(),
        );
        let Some(image) = cache.get_image(fs, key).as_ref() else {
            continue;
        };
        f(
            key,
            image,
            x + image.placement.left,
            y - image.placement.top,
        );
    }
}
