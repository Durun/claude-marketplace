//! レイヤーの見える範囲（切り抜きと角丸の矩形のマスク）と、音のフェード。
//!
//! 切り抜きとマスクの座標はレイヤーの座標（変形の前の、コンポジションの画素。`Transform.anchor` と同じ）。
//! 素材を `fit: none` で置いたレイヤーでは素材の画素と同じになる。

use serde::{Deserialize, Serialize};

use mm_types::{ParamValue, RationalTime};

use crate::param::Param;

use super::{Mask, MaskId, MaskMode};

/// 切り抜き。レイヤーの画像の縁から内側へ削る幅（画素）。
/// DaVinci Resolve の Inspector の Cropping と Premiere Pro の Crop と同じく上下左右を別々に持ち、どれもキーフレームできる。
/// 無い辺は削らない。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Crop {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Param>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Param>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Param>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Param>,
}

impl Crop {
    /// どの辺も削らないか。
    pub fn is_empty(&self) -> bool {
        self.params().next().is_none()
    }

    /// 持っているパラメータの並び（検証と操作が巡る）。
    pub fn params(&self) -> impl Iterator<Item = &Param> {
        [&self.left, &self.top, &self.right, &self.bottom]
            .into_iter()
            .flatten()
    }
}

/// 角丸の矩形のマスクの形。DaVinci Resolve（Fusion）の Rectangle Mask と geneva の `mask` の `rect` に合わせ、
/// 位置・大きさ・角の半径をパラメータで持つ（どれもキーフレームできる）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MaskRect {
    /// 矩形の左上（Vec2）。
    pub position: Param,
    /// 矩形の幅と高さ（Vec2）。
    pub size: Param,
    /// 角の半径（Number）。短い辺の半分で頭打ちにする。
    #[serde(default = "zero")]
    pub radius: Param,
}

impl MaskRect {
    /// 固定の値の矩形。
    pub fn new(position: [f64; 2], size: [f64; 2], radius: f64) -> Self {
        Self {
            position: Param::constant(ParamValue::Vec2(position)),
            size: Param::constant(ParamValue::Vec2(size)),
            radius: Param::constant(ParamValue::Number(radius)),
        }
    }

    /// 持っているパラメータの並び。
    pub fn params(&self) -> impl Iterator<Item = &Param> {
        [&self.position, &self.size, &self.radius].into_iter()
    }
}

/// 頂点の列のマスクの既定（空）。serde の `default` に渡す形。
pub fn empty_path() -> Param {
    Param::constant(ParamValue::Json(serde_json::Value::Array(Vec::new())))
}

/// 0（Number）。serde の `default` に渡す形。
pub fn zero() -> Param {
    Param::constant(ParamValue::Number(0.0))
}

/// 1（Number）。serde の `default` に渡す形。
pub fn one() -> Param {
    Param::constant(ParamValue::Number(1.0))
}

impl Mask {
    /// 角丸の矩形のマスク（足す、反転なし、ぼかしなし、不透明）。
    pub fn rect(id: impl Into<String>, rect: MaskRect) -> Self {
        Self {
            id: MaskId::new(id),
            path: empty_path(),
            rect: Some(rect),
            mode: MaskMode::Add,
            inverted: false,
            feather: zero(),
            opacity: one(),
        }
    }

    /// 持っているパラメータの並び（検証と操作が巡る）。
    pub fn params(&self) -> impl Iterator<Item = &Param> {
        [&self.path, &self.feather, &self.opacity]
            .into_iter()
            .chain(self.rect.iter().flat_map(MaskRect::params))
    }
}

/// 音のフェード。DaVinci Resolve と Final Cut Pro のクリップのフェードのハンドルと同じく、音量のキーフレームとは別に
/// レイヤーの先頭（`fade_in`）と末尾（`fade_out`）に持ち、音量に掛ける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Fade {
    /// フェードの長さ（コンポジションの時間）。正。
    pub duration: RationalTime,
    #[serde(default)]
    pub curve: FadeCurve,
}

impl Fade {
    /// 振幅の直線のフェード。
    pub fn linear(duration: RationalTime) -> Self {
        Self {
            duration,
            curve: FadeCurve::Linear,
        }
    }
}

/// フェードの曲線。進み具合 `p`（無音の側が 0、鳴る側が 1）から振幅の倍率を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FadeCurve {
    /// 振幅の直線（`p`）。geneva と Final Cut Pro の Linear。
    #[default]
    Linear,
    /// 等パワー（`sin(p π/2)`）。重なった 2 つのフェードの和の大きさが保たれる。Final Cut Pro の +3dB。
    EqualPower,
    /// dB の直線。-60 dB から 0 dB まで、`p = 0` だけ無音。
    Db,
}
