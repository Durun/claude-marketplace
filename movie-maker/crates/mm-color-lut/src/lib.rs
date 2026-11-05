//! 色の変換の逃げ道。OCIO が無くても `mm-color-api` の契約を満たす。
//!
//! - [`LutColor`]: `lin_rec709`（作業空間）、`srgb`、`bt709` の相互変換。式を WGSL に書く。
//! - [`Cdl`]: ASC CDL。WGSL を返す。
//! - [`Cube3d`]: `.cube` の 3D LUT。`TransformProgram::Lut3d` と、それを引く WGSL（[`LUT3D_APPLY_WGSL`]）。
//! - [`bake_lut3d`]: 任意の変換を 3D LUT に焼く（OCIO の逃げ道の道具）。

mod bake;
mod cdl;
mod cube;
#[cfg(feature = "gpu-eval")]
pub mod gpu_eval;
pub mod transfer;

use mm_color_api::{ColorTransform, TransformProgram};
use mm_types::{ColorSpace, MmError, Result};

pub use bake::{bake_lut3d, LUT3D_APPLY_WGSL};
pub use cdl::Cdl;
pub use cube::{sample_trilinear, Cube3d};

/// 実装の名前。
pub const LUT_ID: &str = "lut";

/// 知っている空間。
#[derive(Clone, Copy, PartialEq)]
enum Space {
    Working,
    Srgb,
    Bt709,
}

impl Space {
    fn of(cs: &ColorSpace) -> Result<Self> {
        match cs.0.as_str() {
            "lin_rec709" => Ok(Self::Working),
            "srgb" => Ok(Self::Srgb),
            "bt709" => Ok(Self::Bt709),
            other => Err(MmError::Unsupported(format!("色空間 {other}"))),
        }
    }

    fn decode(self, v: f32) -> f32 {
        match self {
            Self::Working => v,
            Self::Srgb => transfer::srgb_to_linear(v),
            Self::Bt709 => transfer::bt709_to_linear(v),
        }
    }

    fn encode(self, v: f32) -> f32 {
        match self {
            Self::Working => v,
            Self::Srgb => transfer::linear_to_srgb(v),
            Self::Bt709 => transfer::linear_to_bt709(v),
        }
    }

    /// 符号化値 `arg` を線形にする WGSL の式。
    fn wgsl_to_linear(self, arg: &str) -> String {
        match self {
            Self::Working => arg.to_owned(),
            Self::Srgb => format!("mm_srgb_to_linear({arg})"),
            Self::Bt709 => format!("mm_bt709_to_linear({arg})"),
        }
    }

    /// 線形の値 `arg` を符号化する WGSL の式。
    fn wgsl_from_linear(self, arg: &str) -> String {
        match self {
            Self::Working => arg.to_owned(),
            Self::Srgb => format!("mm_linear_to_srgb({arg})"),
            Self::Bt709 => format!("mm_linear_to_bt709({arg})"),
        }
    }
}

/// 伝達関数だけの色の変換。3 つの空間は primaries が同じ（Rec.709）なので、チャンネルごとに閉じる。
#[derive(Debug, Clone, Copy, Default)]
pub struct LutColor;

impl LutColor {
    /// 変換を `size` 点の格子に焼く。
    pub fn bake(&self, from: &ColorSpace, to: &ColorSpace, size: u32) -> Result<TransformProgram> {
        bake_lut3d(size, |rgb| self.apply(from, to, rgb))
    }
}

impl ColorTransform for LutColor {
    fn id(&self) -> &'static str {
        LUT_ID
    }

    fn program(&self, from: &ColorSpace, to: &ColorSpace) -> Result<TransformProgram> {
        let (a, b) = (Space::of(from)?, Space::of(to)?);
        if a == b {
            return Ok(TransformProgram::Identity);
        }
        let chain = b.wgsl_from_linear(&a.wgsl_to_linear("v"));
        Ok(TransformProgram::Wgsl {
            source: format!(
                "{}fn mm_channel(v: f32) -> f32 {{ return {chain}; }}
                 fn mm_color_transform(c: vec3<f32>) -> vec3<f32> {{
                     return vec3<f32>(mm_channel(c.x), mm_channel(c.y), mm_channel(c.z));
}}
",
                transfer::TRANSFER_WGSL
            ),
        })
    }

    fn apply(&self, from: &ColorSpace, to: &ColorSpace, rgb: [f32; 3]) -> Result<[f32; 3]> {
        let (a, b) = (Space::of(from)?, Space::of(to)?);
        Ok(rgb.map(|v| b.encode(a.decode(v))))
    }

    fn known_spaces(&self) -> Vec<ColorSpace> {
        vec![
            ColorSpace::working(),
            ColorSpace::srgb(),
            ColorSpace::bt709(),
        ]
    }
}
