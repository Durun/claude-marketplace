//! 色空間の変換の口。レンダラーは [`ColorTransform::program`] で GPU の形を受け取り、
//! 契約テストと CPU の経路は [`ColorTransform::apply`] で同じ変換を数で確かめる。
//!
//! # 色空間の名前の決まり
//!
//! 小文字のスネークケース。レンジ（tv / full）は名前に入れない（デコーダーが RGB へ変換する時点で展開済み）。
//!
//! | 名前 | 伝達関数 | 原色 |
//! |---|---|---|
//! | `lin_rec709` | 線形（作業色空間） | Rec.709 |
//! | `srgb` | IEC 61966-2-1 の区分（静止画の既定） | Rec.709 |
//! | `bt709` | Rec.1886 の表示の式（`linear = v^2.4`、映像の既定。OCIO の `Rec.1886 Rec.709 - Display`） | Rec.709 |
//! | `bt709_oetf` | BT.709 のカメラの OETF（シーン参照の素材だけ） | Rec.709 |
//! | `bt601` | Rec.1886 の表示の式 | Rec.601（SMPTE 170M）。作業色空間には Rec.709 と同じ原色として扱う |
//! | `bt2020` | Rec.1886 の表示の式 | Rec.2020（SDR）。作業色空間へは [`REC2020_TO_REC709`] で変換する |
//!
//! 原色の扱いの理由は RFC 0005 の「bt601 と bt2020 の扱い」。
//!
//! 契約（`mm_testkit::color::contract`）:
//! - 往復（from → to → from）が許容差 1e-3 に入る。
//! - 作業色空間から作業色空間への変換は恒等。
//! - 符号化値 (1,1,1) が白になる空間（名前の表の全部、[`UNIT_WHITE_SPACES`]）では、(1,1,1) は作業色空間でも白。
//!   それ以外の空間（`acescg` など）は白の確認を飛ばし、往復と `program` だけ確かめる。
//! - `program` と `apply` は同じ変換を表す（実装の組で見本の絵が守る）。

use serde::{Deserialize, Serialize};

use mm_types::{ColorSpace, Result};

/// 符号化値 (1,1,1) が白になる空間の名前（名前の表の全部）。契約テストはこの空間だけ白を確かめる。
pub const UNIT_WHITE_SPACES: &[&str] = &[
    "lin_rec709",
    "srgb",
    "bt709",
    "bt709_oetf",
    "bt601",
    "bt2020",
];

/// 線形の Rec.2020（D65）を線形の Rec.709 にする 3x3 行列（BT.2407）。行ごとの和は 1 で、白は白のまま。
/// 結果は 0..1 の外に出る（広色域の色は Rec.709 では負になる）ので、切らずに持ち回る。
pub const REC2020_TO_REC709: [[f32; 3]; 3] = [
    [1.660_491, -0.587_641, -0.072_85],
    [-0.124_55, 1.132_9, -0.008_349],
    [-0.018_151, -0.100_579, 1.118_73],
];

/// 線形の Rec.709 を線形の Rec.2020 にする 3x3 行列。[`REC2020_TO_REC709`] の逆。
pub const REC709_TO_REC2020: [[f32; 3]; 3] = [
    [0.627_404, 0.329_283, 0.043_313],
    [0.069_097, 0.919_540, 0.011_362],
    [0.016_391, 0.088_013, 0.895_595],
];

/// GPU で当てる変換の形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformProgram {
    /// 何もしない。レンダラーはパスを飛ばせる。
    Identity,
    /// WGSL の関数 `fn mm_color_transform(c: vec3<f32>) -> vec3<f32>` を含むソース。
    Wgsl { source: String },
    /// 3D LUT。`size^3 * 3` の f32、R が最も速く変わる。入力は 0..=1 に切る。
    Lut3d { size: u32, data: Vec<f32> },
}

/// 色の変換。入力の色空間 → 作業色空間 → 表示・出力。
pub trait ColorTransform: Send + Sync {
    /// 実装の名前（`ocio`、`lut`、見本は `sample`）。
    fn id(&self) -> &'static str;
    /// `from` から `to` への GPU の形。知らない色空間は `Err(Unsupported)`。
    fn program(&self, from: &ColorSpace, to: &ColorSpace) -> Result<TransformProgram>;
    /// 同じ変換を CPU で 1 画素に当てる。線形の RGB（アルファは触らない）。
    fn apply(&self, from: &ColorSpace, to: &ColorSpace, rgb: [f32; 3]) -> Result<[f32; 3]>;
    /// この実装が知っている色空間の名前。
    fn known_spaces(&self) -> Vec<ColorSpace>;
}
