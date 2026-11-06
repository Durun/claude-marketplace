//! 色の変換のフォールバック。OCIO が無くても `mm-color-api` の契約を満たす。
//!
//! - [`LutColor`]: `lin_rec709`（作業色空間）、`srgb`、`bt709`（Rec.1886）、`bt709_oetf`、`bt601`（`bt709` と同じ）、
//!   `bt2020`（Rec.1886 と Rec.2020 の原色）の相互変換。式を WGSL に書く。
//! - [`Cdl`]: ASC CDL。WGSL を返す。
//! - [`Cube3d`]: `.cube` の 3D LUT。`TransformProgram::Lut3d` と、それを引く WGSL（[`LUT3D_APPLY_WGSL`]）。
//! - [`bake_lut3d`]: 任意の変換を 3D LUT にベイクする（OCIO のフォールバックのためのツール）。

mod bake;
mod cdl;
mod cube;
#[cfg(feature = "gpu-eval")]
pub mod gpu_eval;
mod space;
pub mod transfer;

use mm_color_api::{ColorTransform, TransformProgram};
use mm_types::{ColorSpace, Result};

pub use bake::{bake_lut3d, LUT3D_APPLY_WGSL};
pub use cdl::Cdl;
pub use cube::{sample_trilinear, Cube3d};

use space::Space;

/// 実装の名前。
pub const LUT_ID: &str = "lut";

/// 伝達関数と、`bt2020` のときだけ 3x3 の原色の変換をする色の変換。他は原色が Rec.709 なので、
/// チャンネルごとに閉じる。
#[derive(Debug, Clone, Copy, Default)]
pub struct LutColor;

impl LutColor {
    /// 変換を `size` 点の格子にベイクする。
    pub fn bake(&self, from: &ColorSpace, to: &ColorSpace, size: u32) -> Result<TransformProgram> {
        bake_lut3d(size, |rgb| self.apply(from, to, rgb))
    }
}

/// 原色が同じ組の WGSL。チャンネルごとにデコードしてエンコードする。
fn channel_wgsl(a: Space, b: Space) -> String {
    let chain = b.wgsl_from_linear(&a.wgsl_to_linear("v"));
    format!(
        "fn mm_channel(v: f32) -> f32 {{ return {chain}; }}
fn mm_color_transform(c: vec3<f32>) -> vec3<f32> {{
    return vec3<f32>(mm_channel(c.x), mm_channel(c.y), mm_channel(c.z));
}}
"
    )
}

/// 原色が違う組の WGSL。線形にして 3x3 を掛けてからエンコードする。
fn matrix_wgsl(a: Space, b: Space, m: &space::Matrix) -> String {
    let (dec, enc) = (a.wgsl_to_linear("v"), b.wgsl_from_linear("v"));
    let mixed = space::wgsl_mul(m, "lin");
    format!(
        "fn mm_decode(v: f32) -> f32 {{ return {dec}; }}
fn mm_encode(v: f32) -> f32 {{ return {enc}; }}
fn mm_color_transform(c: vec3<f32>) -> vec3<f32> {{
    let lin = vec3<f32>(mm_decode(c.x), mm_decode(c.y), mm_decode(c.z));
    let m = {mixed};
    return vec3<f32>(mm_encode(m.x), mm_encode(m.y), mm_encode(m.z));
}}
"
    )
}

impl ColorTransform for LutColor {
    fn id(&self) -> &'static str {
        LUT_ID
    }

    fn program(&self, from: &ColorSpace, to: &ColorSpace) -> Result<TransformProgram> {
        let (a, b) = (Space::of(from)?, Space::of(to)?);
        if a.is_same(b) {
            return Ok(TransformProgram::Identity);
        }
        let body = match a.primaries_matrix(b) {
            None => channel_wgsl(a, b),
            Some(m) => matrix_wgsl(a, b, &m),
        };
        Ok(TransformProgram::Wgsl {
            source: format!("{}{body}", transfer::TRANSFER_WGSL),
        })
    }

    fn apply(&self, from: &ColorSpace, to: &ColorSpace, rgb: [f32; 3]) -> Result<[f32; 3]> {
        let (a, b) = (Space::of(from)?, Space::of(to)?);
        let linear = rgb.map(|v| a.decode(v));
        let mixed = a
            .primaries_matrix(b)
            .map_or(linear, |m| space::mul(&m, linear));
        Ok(mixed.map(|v| b.encode(v)))
    }

    fn known_spaces(&self) -> Vec<ColorSpace> {
        vec![
            ColorSpace::working(),
            ColorSpace::srgb(),
            ColorSpace::bt709(),
            ColorSpace::bt709_oetf(),
            ColorSpace::bt601(),
            ColorSpace::bt2020(),
        ]
    }
}
