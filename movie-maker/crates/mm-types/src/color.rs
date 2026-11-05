//! 色。中間の絵は線形の Rec.709、premultiplied のアルファで持つ。

use serde::{Deserialize, Serialize};

/// 線形、premultiplied の RGBA。各成分は 0..=1 の範囲が基本だが、HDR では 1 を超えてよい。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    /// 完全な透明。
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    /// 不透明の黒。
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    /// 不透明の白。
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);

    /// premultiplied の値をそのまま持つ。
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// straight（アルファを掛けていない）の値から premultiplied にする。
    pub fn from_straight(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::new(r * a, g * a, b * a, a)
    }

    /// `src over self`（W3C Compositing and Blending の source-over）。
    pub fn over(self, src: Self) -> Self {
        let k = 1.0 - src.a;
        Self::new(
            src.r + self.r * k,
            src.g + self.g * k,
            src.b + self.b * k,
            src.a + self.a * k,
        )
    }

    /// 全成分に倍率を掛ける（不透明度）。
    pub fn scaled(self, k: f32) -> Self {
        Self::new(self.r * k, self.g * k, self.b * k, self.a * k)
    }

    /// 成分ごとの差の最大。テストの許容差に使う。
    pub fn max_diff(self, other: Self) -> f32 {
        (self.r - other.r)
            .abs()
            .max((self.g - other.g).abs())
            .max((self.b - other.b).abs())
            .max((self.a - other.a).abs())
    }
}

/// 色空間の名前。OCIO の名前をそのまま持つ（`bt709`、`srgb`、`lin_rec709` など）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ColorSpace(pub String);

impl ColorSpace {
    /// 作業色空間。線形の Rec.709。
    pub fn working() -> Self {
        Self("lin_rec709".into())
    }

    /// 映像の既定。Rec.1886 の表示の式（べき 2.4）、Rec.709 の原色。
    pub fn bt709() -> Self {
        Self("bt709".into())
    }

    /// BT.709 のカメラの OETF、Rec.709 の原色。シーン参照の素材だけに使う。
    pub fn bt709_oetf() -> Self {
        Self("bt709_oetf".into())
    }

    /// Rec.1886 の表示の式、Rec.601（SMPTE 170M）の原色。
    pub fn bt601() -> Self {
        Self("bt601".into())
    }

    /// Rec.1886 の表示の式、Rec.2020 の原色（SDR）。
    pub fn bt2020() -> Self {
        Self("bt2020".into())
    }

    /// 画像の既定の入力。
    pub fn srgb() -> Self {
        Self("srgb".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 不透明を重ねると上の色になる() {
        let red = Rgba::from_straight(1.0, 0.0, 0.0, 1.0);
        assert_eq!(Rgba::WHITE.over(red), red);
    }

    #[test]
    fn 透明を重ねても変わらない() {
        assert_eq!(Rgba::WHITE.over(Rgba::TRANSPARENT), Rgba::WHITE);
    }
}
