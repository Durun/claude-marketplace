//! 行組みをもう一度引いて、字形を CPU で RGBA に描く。

use cosmic_text::{Buffer, FontSystem, SwashCache, SwashContent};

use mm_text_api::{ShapedText, TextStyle};
use mm_types::{Rgba, RgbaImage, Size};

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

/// `buffer` の字形を `scale` 倍で `size` の絵に描く。`origin` は 1 倍の座標での絵の左上。
pub(crate) fn draw(
    fs: &mut FontSystem,
    cache: &mut SwashCache,
    buffer: &Buffer,
    style: &TextStyle,
    scale: f32,
    origin: (f32, f32),
    size: Size,
) -> RgbaImage {
    let mut img = RgbaImage::transparent(size);
    for run in buffer.layout_runs() {
        let offset = (-origin.0 * scale, (run.line_y - origin.1) * scale);
        for glyph in run.glyphs {
            let phys = glyph.physical(offset, scale);
            let Some(image) = cache.get_image(fs, phys.cache_key).as_ref() else {
                continue;
            };
            let (w, h) = (
                image.placement.width as usize,
                image.placement.height as usize,
            );
            let left = phys.x + image.placement.left;
            let top = phys.y - image.placement.top;
            for row in 0..h {
                for col in 0..w {
                    let Some(src) =
                        pixel_color(image.content, &image.data, row * w + col, style.color)
                    else {
                        continue;
                    };
                    let (x, y) = (left + col as i32, top + row as i32);
                    if x < 0 || y < 0 {
                        continue;
                    }
                    let (x, y) = (x as u32, y as u32);
                    let under = img.pixel(x, y);
                    img.set_pixel(x, y, under.over(src));
                }
            }
        }
    }
    img
}
