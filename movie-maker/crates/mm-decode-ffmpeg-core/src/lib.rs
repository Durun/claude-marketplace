//! `mm-decode-ffmpeg`、`mm-decode-d3d12va`、`mm-decode-cuvid` が共有する、FFmpeg の CPU の部分
//! （実装の共有部。`docs/rfcs/0007-decode-shared-crates.md`）。GPU には依存しない。
//!
//! - [`timeline`]: パケットの PTS の索引。コマの番号、`MediaInfo`、シークの判断を決める。
//! - [`reader`]: 復号器からコマを取り出し、番号を付けて、シークで位置を合わせる。
//! - [`matrix`]: YUV の行列、レンジ、扱えない伝達特性の判定。
//!
//! feature `ffmpeg` を入れたときだけ中身をビルドする（FFmpeg の開発ファイルが要るため）。
#![cfg(feature = "ffmpeg")]

pub mod matrix;
pub mod reader;
pub mod timeline;
