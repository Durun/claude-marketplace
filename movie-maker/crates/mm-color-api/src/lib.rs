//! 色空間の変換の口。描画器は [`ColorTransform::program`] で GPU の形を受け取り、
//! 契約テストと CPU の経路は [`ColorTransform::apply`] で同じ変換を数で確かめる。
//!
//! 契約（`mm_testkit::color::contract`）:
//! - 往復（from → to → from）が許容差 1e-3 に入る。
//! - 作業空間から作業空間への変換は恒等。
//! - Rec.709 の白 (1,1,1) は作業空間でも白。
//! - `program` と `apply` は同じ変換を表す（実装の組で見本の絵が守る）。

use serde::{Deserialize, Serialize};

use mm_types::{ColorSpace, Result};

/// GPU で当てる変換の形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformProgram {
    /// 何もしない。描画器はパスを飛ばせる。
    Identity,
    /// WGSL の関数 `fn mm_color_transform(c: vec3<f32>) -> vec3<f32>` を含むソース。
    Wgsl { source: String },
    /// 3D LUT。`size^3 * 3` の f32、R が最も速く変わる。入力は 0..=1 に切る。
    Lut3d { size: u32, data: Vec<f32> },
}

/// 色の変換。入力の色空間 → 作業空間 → 表示・出力。
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
