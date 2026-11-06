//! `mm-color-lut` が知る色空間を、伝達関数と原色に分けて持つ。

use mm_color_api::{REC2020_TO_REC709, REC709_TO_REC2020};
use mm_types::{ColorSpace, MmError, Result};

use crate::transfer;

/// 3x3 行列（行ごと）。
pub(crate) type Matrix = [[f32; 3]; 3];

/// 伝達関数。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transfer {
    Linear,
    Srgb,
    Bt1886,
    Bt709Oetf,
}

/// 原色。作業空間は Rec.709。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Primaries {
    Rec709,
    Rec2020,
}

/// 知っている空間。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Space {
    transfer: Transfer,
    primaries: Primaries,
}

const fn space(transfer: Transfer, primaries: Primaries) -> Space {
    Space {
        transfer,
        primaries,
    }
}

impl Space {
    /// `bt601` は Rec.709 と同じ原色として扱う（RFC 0005）ので `bt709` と同じ値になる。
    pub(crate) fn of(cs: &ColorSpace) -> Result<Self> {
        use Primaries::{Rec2020, Rec709};
        use Transfer::{Bt1886, Bt709Oetf, Linear, Srgb};
        match cs.0.as_str() {
            "lin_rec709" => Ok(space(Linear, Rec709)),
            "srgb" => Ok(space(Srgb, Rec709)),
            "bt709" | "bt601" => Ok(space(Bt1886, Rec709)),
            "bt709_oetf" => Ok(space(Bt709Oetf, Rec709)),
            "bt2020" => Ok(space(Bt1886, Rec2020)),
            other => Err(MmError::Unsupported(format!("色空間 {other}"))),
        }
    }

    /// 同じ伝達関数と原色なら、変換は何もしない。
    pub(crate) fn is_same(self, other: Self) -> bool {
        self == other
    }

    /// 符号化値から線形へ（チャンネルごと）。
    pub(crate) fn decode(self, v: f32) -> f32 {
        match self.transfer {
            Transfer::Linear => v,
            Transfer::Srgb => transfer::srgb_to_linear(v),
            Transfer::Bt1886 => transfer::bt1886_to_linear(v),
            Transfer::Bt709Oetf => transfer::bt709_to_linear(v),
        }
    }

    /// 線形から符号化値へ（チャンネルごと）。
    pub(crate) fn encode(self, v: f32) -> f32 {
        match self.transfer {
            Transfer::Linear => v,
            Transfer::Srgb => transfer::linear_to_srgb(v),
            Transfer::Bt1886 => transfer::linear_to_bt1886(v),
            Transfer::Bt709Oetf => transfer::linear_to_bt709(v),
        }
    }

    /// 符号化値 `arg` を線形にする WGSL の式。
    pub(crate) fn wgsl_to_linear(self, arg: &str) -> String {
        match self.transfer {
            Transfer::Linear => arg.to_owned(),
            Transfer::Srgb => format!("mm_srgb_to_linear({arg})"),
            Transfer::Bt1886 => format!("mm_bt1886_to_linear({arg})"),
            Transfer::Bt709Oetf => format!("mm_bt709_to_linear({arg})"),
        }
    }

    /// 線形の値 `arg` を符号化する WGSL の式。
    pub(crate) fn wgsl_from_linear(self, arg: &str) -> String {
        match self.transfer {
            Transfer::Linear => arg.to_owned(),
            Transfer::Srgb => format!("mm_linear_to_srgb({arg})"),
            Transfer::Bt1886 => format!("mm_linear_to_bt1886({arg})"),
            Transfer::Bt709Oetf => format!("mm_linear_to_bt709({arg})"),
        }
    }

    /// 線形のままこの空間の原色から `to` の原色へ移す行列。原色が同じなら `None`。
    pub(crate) fn primaries_matrix(self, to: Self) -> Option<Matrix> {
        match (self.primaries, to.primaries) {
            (Primaries::Rec2020, Primaries::Rec709) => Some(REC2020_TO_REC709),
            (Primaries::Rec709, Primaries::Rec2020) => Some(REC709_TO_REC2020),
            _ => None,
        }
    }
}

/// 行列を RGB に掛ける。
pub(crate) fn mul(m: &Matrix, v: [f32; 3]) -> [f32; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

/// 行列を掛ける WGSL の文（`lin` を `vec3<f32>` に直す）。
pub(crate) fn wgsl_mul(m: &Matrix, arg: &str) -> String {
    let rows = m.map(|r| format!("dot(vec3<f32>({:?}, {:?}, {:?}), {arg})", r[0], r[1], r[2]));
    format!("vec3<f32>({}, {}, {})", rows[0], rows[1], rows[2])
}
