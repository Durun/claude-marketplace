//! `mm-color-ocio` の結合テスト。リンクを 1 回にするため、1 つの実行ファイルにまとめる（docs/ci.md）。

#[cfg(feature = "native")]
mod accuracy;
#[cfg(feature = "native")]
mod contract;
#[cfg(feature = "native")]
mod timing;
