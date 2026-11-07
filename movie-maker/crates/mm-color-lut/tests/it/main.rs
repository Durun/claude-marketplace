//! `mm-color-lut` の結合テスト。リンクを 1 回にするため、1 つの実行ファイルにまとめる（docs/ci.md）。

mod contract;
#[cfg(feature = "gpu-eval")]
mod gpu;
