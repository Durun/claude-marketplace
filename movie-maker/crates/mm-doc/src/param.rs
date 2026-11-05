//! パラメータとキーフレーム。評価（補間とイージング）は `mm-eval` が行い、ここは形だけ持つ。

use serde::{Deserialize, Serialize};

use mm_types::{ParamValue, RationalTime};

/// キーフレームの補間。区間 `[k_i, k_{i+1})` の形は始点 `k_i` の補間で決まる。
/// 数の型（数、2・3 次元の数、色）だけ `Linear` と `Bezier` が効き、ほかはホールド。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// 次のキーフレームまで値を保つ。
    Hold,
    /// 進み具合に [`Ease`] を掛けて直線で補間する。
    Linear,
    /// 進み具合を [`CubicBezier`]（CSS の `cubic-bezier` と同じ）で曲げる。
    Bezier,
}

/// `Linear` の進み具合に掛けるイージングの型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    /// 等速。
    #[default]
    None,
    /// ゆっくり始まる（2 次）。
    In,
    /// ゆっくり終わる（2 次）。
    Out,
    /// 両端がゆっくり（2 次を貼り合わせ）。
    InOut,
}

/// 進み具合 `p ∈ [0,1]` を曲げる 3 次ベジェ。`(0,0)` と `(1,1)` を端とし、制御点が `(x1,y1)`、`(x2,y2)`。
/// `x1`、`x2` は `[0,1]` に収める（時間が単調になる）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CubicBezier {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl CubicBezier {
    /// CSS の `ease-in-out`（0.42, 0, 0.58, 1）。`Bezier` の制御点が無いときの既定。
    pub const EASE_IN_OUT: Self = Self {
        x1: 0.42,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };

    /// 制御点の x が `[0,1]` に収まるか。
    pub fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.x1) && (0.0..=1.0).contains(&self.x2)
    }
}

impl Default for CubicBezier {
    fn default() -> Self {
        Self::EASE_IN_OUT
    }
}

/// キーフレーム。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: RationalTime,
    pub value: ParamValue,
    pub interpolation: Interpolation,
    /// `Linear` の進み具合に掛ける。
    #[serde(default)]
    pub ease: Ease,
    /// `Bezier` の制御点。無ければ [`CubicBezier::EASE_IN_OUT`]。
    #[serde(default)]
    pub bezier: Option<CubicBezier>,
}

impl Keyframe {
    /// 指定の補間で、イージング無し。
    pub fn new(time: RationalTime, value: ParamValue, interpolation: Interpolation) -> Self {
        Self {
            time,
            value,
            interpolation,
            ease: Ease::None,
            bezier: None,
        }
    }
}

/// パラメータ。固定の値か、キーフレームの列（時刻の昇順）。
///
/// キーフレームがあるときは `value` を使わず、最初のキーフレームより前は最初の値、
/// 最後より後は最後の値になる（`mm-eval`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub value: ParamValue,
    #[serde(default)]
    pub keyframes: Vec<Keyframe>,
}

impl Param {
    /// 固定の値。
    pub fn constant(value: ParamValue) -> Self {
        Self {
            value,
            keyframes: Vec::new(),
        }
    }

    /// キーフレームを持つか。
    pub fn is_animated(&self) -> bool {
        !self.keyframes.is_empty()
    }
}
