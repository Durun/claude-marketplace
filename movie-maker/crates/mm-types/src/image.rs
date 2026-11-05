//! CPU 側の絵。GPU のテクスチャ（Rgba16Float）と同じ意味（線形、premultiplied）を f32 で持つ。
//! 見本の実装と契約テスト、CPU で描く種類（Lottie、SVG）のアップロード元に使う。

use crate::color::Rgba;
use crate::error::{MmError, Result};
use crate::geom::{Rect, Size};

/// 1 画素の成分の数。
pub const CHANNELS: usize = 4;

/// RGBA f32 の絵。行は上から、画素は左から。
#[derive(Debug, Clone, PartialEq)]
pub struct RgbaImage {
    size: Size,
    data: Vec<f32>,
}

impl RgbaImage {
    /// 1 色で埋めた絵。
    pub fn filled(size: Size, color: Rgba) -> Self {
        let mut data = Vec::with_capacity(size.area() * CHANNELS);
        for _ in 0..size.area() {
            data.extend_from_slice(&[color.r, color.g, color.b, color.a]);
        }
        Self { size, data }
    }

    /// 透明の絵。
    pub fn transparent(size: Size) -> Self {
        Self::filled(size, Rgba::TRANSPARENT)
    }

    /// 既存の画素の列から。長さが合わなければ `InvalidArgument`。
    pub fn from_raw(size: Size, data: Vec<f32>) -> Result<Self> {
        if data.len() != size.area() * CHANNELS {
            return Err(MmError::InvalidArgument(format!(
                "RgbaImage: {}x{} に対して長さ {}",
                size.width,
                size.height,
                data.len()
            )));
        }
        Ok(Self { size, data })
    }

    /// 大きさ。
    pub fn size(&self) -> Size {
        self.size
    }

    /// 幅。
    pub fn width(&self) -> u32 {
        self.size.width
    }

    /// 高さ。
    pub fn height(&self) -> u32 {
        self.size.height
    }

    /// 画素の列（RGBA の繰り返し）。
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    fn offset(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.size.width && y < self.size.height)
            .then(|| (y as usize * self.size.width as usize + x as usize) * CHANNELS)
    }

    /// 画素。範囲の外は透明。
    pub fn pixel(&self, x: u32, y: u32) -> Rgba {
        match self.offset(x, y) {
            Some(i) => Rgba::new(
                self.data[i],
                self.data[i + 1],
                self.data[i + 2],
                self.data[i + 3],
            ),
            None => Rgba::TRANSPARENT,
        }
    }

    /// 画素を書く。範囲の外は無視。
    pub fn set_pixel(&mut self, x: u32, y: u32, c: Rgba) {
        if let Some(i) = self.offset(x, y) {
            self.data[i..i + CHANNELS].copy_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }

    /// 矩形を 1 色で塗る（source-over）。画面の外にはみ出した分は切る。
    pub fn fill_rect(&mut self, rect: Rect, color: Rgba) {
        let clipped = rect.intersect(&self.size.to_rect());
        if clipped.is_empty() {
            return;
        }
        let x0 = clipped.x.floor() as u32;
        let y0 = clipped.y.floor() as u32;
        let x1 = clipped.right().ceil() as u32;
        let y1 = clipped.bottom().ceil() as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let under = self.pixel(x, y);
                self.set_pixel(x, y, under.over(color));
            }
        }
    }

    /// `src` を `(dx, dy)` に source-over で重ねる。`opacity` は 0..=1。
    pub fn blit_over(&mut self, src: &RgbaImage, dx: i64, dy: i64, opacity: f32) {
        for sy in 0..src.height() {
            let y = dy + sy as i64;
            if y < 0 || y >= self.height() as i64 {
                continue;
            }
            for sx in 0..src.width() {
                let x = dx + sx as i64;
                if x < 0 || x >= self.width() as i64 {
                    continue;
                }
                let s = src.pixel(sx, sy).scaled(opacity);
                let under = self.pixel(x as u32, y as u32);
                self.set_pixel(x as u32, y as u32, under.over(s));
            }
        }
    }

    /// 2 枚の絵の、全画素・全成分の差の最大。大きさが違えば `None`。
    pub fn max_diff(&self, other: &RgbaImage) -> Option<f32> {
        if self.size != other.size {
            return None;
        }
        Some(
            self.data
                .iter()
                .zip(&other.data)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 矩形を塗ると中だけ変わる() {
        let mut img = RgbaImage::transparent(Size::new(4, 4));
        img.fill_rect(Rect::new(1.0, 1.0, 2.0, 2.0), Rgba::WHITE);
        assert_eq!(img.pixel(1, 1), Rgba::WHITE);
        assert_eq!(img.pixel(0, 0), Rgba::TRANSPARENT);
        assert_eq!(img.pixel(3, 3), Rgba::TRANSPARENT);
    }

    #[test]
    fn 重ねると下が透ける() {
        let mut under = RgbaImage::filled(Size::new(1, 1), Rgba::WHITE);
        let over = RgbaImage::filled(Size::new(1, 1), Rgba::from_straight(0.0, 0.0, 0.0, 0.5));
        under.blit_over(&over, 0, 0, 1.0);
        let p = under.pixel(0, 0);
        assert!((p.r - 0.5).abs() < 1e-6 && (p.a - 1.0).abs() < 1e-6);
    }
}
