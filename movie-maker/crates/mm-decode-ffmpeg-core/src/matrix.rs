//! YUV の行列とレンジの宣言、扱えない伝達特性の判定。

use ff::color::{Range, Space, TransferCharacteristic};
use ffmpeg_next as ff;
use mm_types::{ColorSpace, MmError, Result};

/// YUV から RGB への行列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Matrix {
    /// フレームの宣言から。未指定は画の高さで決める（HD 以上は 709）。
    pub fn from_frame(space: Space, height: u32) -> Self {
        match space {
            Space::BT709 => Self::Bt709,
            Space::BT2020NCL | Space::BT2020CL => Self::Bt2020,
            Space::SMPTE170M | Space::BT470BG | Space::FCC | Space::SMPTE240M => Self::Bt601,
            _ if height >= 720 => Self::Bt709,
            _ => Self::Bt601,
        }
    }

    /// 文書に出す色空間の名前。レンジは RGB への変換で展開済みなので名前に入れない。
    pub fn color_space(self) -> ColorSpace {
        match self {
            Self::Bt601 => ColorSpace::bt601(),
            Self::Bt709 => ColorSpace::bt709(),
            Self::Bt2020 => ColorSpace::bt2020(),
        }
    }

    /// 輝度の係数 (Kr, Kb)。
    pub fn coefficients(self) -> (f32, f32) {
        match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
            Self::Bt2020 => (0.2627, 0.0593),
        }
    }
}

/// フレームのレンジの宣言。未指定は tv レンジ。
pub fn is_full_range(range: Range) -> bool {
    matches!(range, Range::JPEG)
}

/// 符号化値に Rec.1886 の式を当てて線形にする経路で扱えない伝達特性（PQ と HLG）なら `Unsupported`。
/// 当てると明るさが大きく狂うので、黙って通さず、どの実装も同じ判定で断る。
pub fn check_transfer(trc: TransferCharacteristic) -> Result<()> {
    match trc {
        TransferCharacteristic::SMPTE2084 => Err(MmError::Unsupported(
            "PQ（SMPTE ST 2084）の素材は扱えない".into(),
        )),
        TransferCharacteristic::ARIB_STD_B67 => Err(MmError::Unsupported(
            "HLG（ARIB STD-B67）の素材は扱えない".into(),
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pq_と_hlg_は断り_ほかは通す() {
        assert!(matches!(
            check_transfer(TransferCharacteristic::SMPTE2084),
            Err(MmError::Unsupported(_))
        ));
        assert!(matches!(
            check_transfer(TransferCharacteristic::ARIB_STD_B67),
            Err(MmError::Unsupported(_))
        ));
        for ok in [
            TransferCharacteristic::BT709,
            TransferCharacteristic::SMPTE170M,
            TransferCharacteristic::Unspecified,
        ] {
            assert!(check_transfer(ok).is_ok(), "{ok:?}");
        }
    }

    #[test]
    fn 行列は宣言を優先し_未指定は高さで決める() {
        assert_eq!(Matrix::from_frame(Space::BT2020NCL, 480), Matrix::Bt2020);
        assert_eq!(Matrix::from_frame(Space::Unspecified, 1080), Matrix::Bt709);
        assert_eq!(Matrix::from_frame(Space::Unspecified, 480), Matrix::Bt601);
    }
}
