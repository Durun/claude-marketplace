//! 縁取り。グリフの被覆率を円盤で太らせて、文字色の下に敷く。

use cosmic_text::SwashContent;
use mm_types::{Rgba, RgbaImage};

/// 縁の端の半透明の幅（画素）。小数の太さでも縁が段にならない。
const EDGE_SOFTNESS: f32 = 0.5;

/// 被覆率の画像。`Mask` のグリフだけが寄与する（絵文字には縁を付けない）。
pub(crate) struct Coverage {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Coverage {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            width: width as usize,
            height: height as usize,
            data: vec![0.0; width as usize * height as usize],
        }
    }

    /// グリフのビットマップの被覆率を、重なる所は大きい方で取り込む。
    pub(crate) fn add_glyph(&mut self, image: &cosmic_text::SwashImage, left: i32, top: i32) {
        if image.content != SwashContent::Mask {
            return;
        }
        let (w, h) = (
            image.placement.width as usize,
            image.placement.height as usize,
        );
        for row in 0..h {
            for col in 0..w {
                let (x, y) = (left + col as i32, top + row as i32);
                if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
                    continue;
                }
                let v = f32::from(image.data[row * w + col]) / 255.0;
                let cell = &mut self.data[y as usize * self.width + x as usize];
                *cell = cell.max(v);
            }
        }
    }

    /// 半径 `radius`（画素）の円盤で太らせた被覆率を、`color` で `img` に塗る。
    pub(crate) fn paint_stroke(&self, img: &mut RgbaImage, color: Rgba, radius: f32) {
        let reach = (radius + EDGE_SOFTNESS).ceil() as i32;
        let offsets = disk_offsets(radius, reach);
        let mut grown = vec![0.0f32; self.data.len()];
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                let v = self.data[y as usize * self.width + x as usize];
                if v > 0.0 {
                    self.spread(&mut grown, (x, y), v, &offsets);
                }
            }
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let v = grown[y * self.width + x];
                if v > 0.0 {
                    let px = Rgba::new(color.r * v, color.g * v, color.b * v, color.a * v);
                    img.set_pixel(x as u32, y as u32, px);
                }
            }
        }
    }

    /// 1 画素の被覆率 `v` を、円盤の各位置へ重みを掛けて大きい方で書き込む。
    fn spread(&self, grown: &mut [f32], at: (i32, i32), v: f32, offsets: &[(i32, i32, f32)]) {
        for &(dx, dy, weight) in offsets {
            let (tx, ty) = (at.0 + dx, at.1 + dy);
            if tx < 0 || ty < 0 || tx as usize >= self.width || ty as usize >= self.height {
                continue;
            }
            let cell = &mut grown[ty as usize * self.width + tx as usize];
            *cell = cell.max(v * weight);
        }
    }
}

/// 円盤の中の相対位置と、端の半透明を表す重み。
fn disk_offsets(radius: f32, reach: i32) -> Vec<(i32, i32, f32)> {
    let mut offsets = Vec::new();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            let weight = (radius + EDGE_SOFTNESS - dist).clamp(0.0, 1.0);
            if weight > 0.0 {
                offsets.push((dx, dy, weight));
            }
        }
    }
    offsets
}
