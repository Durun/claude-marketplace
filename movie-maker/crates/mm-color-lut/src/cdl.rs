//! ASC CDL（slope、offset、power、saturation）。式は ASC CDL v1.2 の通り:
//! `out = clamp(in * slope + offset) ^ power`、その後に Rec.709 の輝度係数で彩度を掛ける。

use serde::{Deserialize, Serialize};

/// 彩度に使う輝度係数（Rec.709）。
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// ASC CDL の 10 個の数。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cdl {
    pub slope: [f32; 3],
    pub offset: [f32; 3],
    pub power: [f32; 3],
    pub saturation: f32,
}

impl Default for Cdl {
    fn default() -> Self {
        Self {
            slope: [1.0; 3],
            offset: [0.0; 3],
            power: [1.0; 3],
            saturation: 1.0,
        }
    }
}

impl Cdl {
    /// 1 画素に当てる。
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        // ASC CDL は power の前で 0 以上に切る。
        let v: [f32; 3] = std::array::from_fn(|i| {
            (rgb[i] * self.slope[i] + self.offset[i])
                .max(0.0)
                .powf(self.power[i])
        });
        let luma = v[0] * LUMA[0] + v[1] * LUMA[1] + v[2] * LUMA[2];
        v.map(|c| luma + (c - luma) * self.saturation)
    }

    /// `fn mm_color_transform(c: vec3<f32>) -> vec3<f32>` を含む WGSL。
    pub fn to_wgsl(&self) -> String {
        let v3 = |a: [f32; 3]| format!("vec3<f32>({:?}, {:?}, {:?})", a[0], a[1], a[2]);
        format!(
            "fn mm_color_transform(c: vec3<f32>) -> vec3<f32> {{\n    \
             let v = pow(max(c * {slope} + {offset}, vec3<f32>(0.0)), {power});\n    \
             let luma = dot(v, vec3<f32>({l0:?}, {l1:?}, {l2:?}));\n    \
             return vec3<f32>(luma) + (v - vec3<f32>(luma)) * {sat:?};\n}}\n",
            slope = v3(self.slope),
            offset = v3(self.offset),
            power = v3(self.power),
            l0 = LUMA[0],
            l1 = LUMA[1],
            l2 = LUMA[2],
            sat = self.saturation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 既定の値は恒等() {
        let out = Cdl::default().apply([0.2, 0.5, 0.8]);
        for (a, b) in out.iter().zip([0.2, 0.5, 0.8]) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn 彩度0は輝度のグレーになる() {
        let cdl = Cdl {
            saturation: 0.0,
            ..Cdl::default()
        };
        let out = cdl.apply([1.0, 0.0, 0.0]);
        assert!((out[0] - 0.2126).abs() < 1e-6 && out[0] == out[1] && out[1] == out[2]);
    }

    #[test]
    fn slopeとoffsetとpowerを順に当てる() {
        let cdl = Cdl {
            slope: [2.0; 3],
            offset: [-0.1; 3],
            power: [2.0; 3],
            saturation: 1.0,
        };
        let out = cdl.apply([0.3; 3]);
        assert!((out[0] - 0.25).abs() < 1e-6);
    }
}
