//! `mm-decode-ffmpeg` の結合テスト。リンクを 1 回にするため、1 つの実行ファイルにまとめる（docs/ci.md）。

#[cfg(feature = "ffmpeg")]
mod common;
#[cfg(feature = "ffmpeg")]
mod contract;
#[cfg(feature = "ffmpeg")]
mod footage;
#[cfg(feature = "gpu")]
mod gpu_upload;
#[cfg(feature = "ffmpeg")]
mod media;
