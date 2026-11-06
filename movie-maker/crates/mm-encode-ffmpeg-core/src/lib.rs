//! FFmpeg を使うエンコーダーの共通実装（`mm-decode-api` の `Encoder` の実装）。
//!
//! コーデックの指定（[`CodecSpec`]）から、映像と音を MP4 か MOV に書く。フレームは線形の RGBA の f32 で受け取り、
//! Rec.1886 の符号化値に直してからエンコーダーの画素形式（Rec.709、tv レンジ）にして押す。レンダラーが GPU で作った NV12
//! （`push_nv12_frames`）は、NV12 を受けるコーデックなら変換せずに押す。フレームの時刻は 1 フレームごとに `1 / fps` 進める
//! （書き出しは CFR）。2 パスの指定は、1 回目に統計を取り、2 回目に書く。
//!
//! 名前からコーデックの指定を決めるのは実装のクレート（`mm-encode-ffmpeg`、`mm-encode-nvenc`）で、このクレートは
//! 指定だけを見る。指定の型は FFmpeg なしでもビルドでき、書く部分は feature `ffmpeg` のときだけビルドする
//! （FFmpeg の開発ファイルが要るため）。

pub mod spec;

pub use spec::{AudioCodec, CodecSpec, Container, Rate, VideoInput};

#[cfg(feature = "ffmpeg")]
mod audio;
#[cfg(feature = "ffmpeg")]
mod transfer;
#[cfg(feature = "ffmpeg")]
mod video;
#[cfg(feature = "ffmpeg")]
mod writer;

#[cfg(feature = "ffmpeg")]
pub use writer::{can_open, encoder_available, init, FfmpegEncoder, SpecResolver};
