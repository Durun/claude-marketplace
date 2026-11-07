//! `mm` の結合テスト。リンクを 1 回にするため、1 つの実行ファイルにまとめる（docs/ci.md）。

#[cfg(feature = "ffmpeg")]
mod caption_burn_in;
#[cfg(feature = "ffmpeg")]
mod captions_loudness;
mod cli;
#[cfg(feature = "ffmpeg")]
#[path = "../../../../crates/mm-decode-ffmpeg/tests/it/common/mod.rs"]
mod common;
#[cfg(feature = "ffmpeg")]
mod cut_words;
#[cfg(feature = "ffmpeg")]
mod ep03;
mod lut_cache;
#[cfg(feature = "ffmpeg")]
mod preview_frame;
#[cfg(feature = "ffmpeg")]
mod render;
#[cfg(feature = "ffmpeg")]
mod shorts;
#[cfg(feature = "ffmpeg")]
mod stage3_e2e;
#[cfg(feature = "ffmpeg")]
mod stage4_e2e;
#[cfg(feature = "ffmpeg")]
mod transcript;
