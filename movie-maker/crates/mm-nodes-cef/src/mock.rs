//! CEF が無い環境の代わりの描画。ページを描かず、ページとパラメータで決まる色の矩形を中央に置く。
//! 置き場所と時刻によらない画素なので、レイヤーの出し入れと合成の確かめには足りる。

use mm_types::{Rect, Rgba, RgbaImage};

use crate::page::{hash_of, FrameRequest};

/// 矩形の不透明度。下の映像が透けて見え、モックだと分かる。
const ALPHA: f32 = 0.8;
/// 矩形が画布に占める割合（縦横とも）。
const SHARE: f32 = 0.5;
/// 色の 1 成分に使うビット。
const CHANNEL_BITS: u32 = 8;
const CHANNEL_MAX: f32 = 255.0;

/// モックの 1 フレーム。
pub fn render(req: &FrameRequest<'_>) -> RgbaImage {
    let size = req.size;
    let mut image = RgbaImage::transparent(size);
    let h = hash_of(&(req.page.hash, &req.page.data_text));
    let channel = |i: u32| ((h >> (i * CHANNEL_BITS)) & 0xff) as f32 / CHANNEL_MAX;
    let color = Rgba::from_straight(channel(0), channel(1), channel(2), ALPHA);
    let (w, hgt) = (size.width as f32, size.height as f32);
    let margin = (1.0 - SHARE) / 2.0;
    let rect = Rect::new(w * margin, hgt * margin, w * SHARE, hgt * SHARE);
    image.fill_rect(rect, color);
    image
}
