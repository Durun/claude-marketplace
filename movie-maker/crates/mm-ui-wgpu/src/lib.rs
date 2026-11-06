//! wgpu のサーフェス上の UI。Program のビューアー（エンジンが返した線形の画像を Rec.1886 で出す）と
//! Timeline（`mm-ui-layout` が落とした矩形を描く）を 1 枚のサーフェスに描き、Timeline のポインターの入力を受ける。
//!
//! 計算は `mm-ui-layout` に置き、このクレートは wgpu の資源と描画だけを持つ。
//! 文字を描く仕組みは持たない（目盛りは 7 セグメントの矩形）。文字は WebView 側のパネルが出す。

mod model;
mod program;
mod quads;
mod ui;

pub use model::{timeline_layer_ids, timeline_model};
pub use program::ProgramViewer;
pub use quads::QuadRenderer;
pub use ui::Ui;
