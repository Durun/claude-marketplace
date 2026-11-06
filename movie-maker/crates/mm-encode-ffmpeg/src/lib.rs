//! FFmpeg のライブラリによるエンコーダー（`mm-decode-api` の `Encoder` の実装）。
//!
//! 扱うコーデックと出力:
//!
//! | `EncodeSettings::codec` | 中身 | コンテナ |
//! |---|---|---|
//! | `h264`、`libx264` | libx264、CRF 18、AAC | MP4 |
//! | `h264_2pass` | libx264 の 2 パス（目標のビットレートは大きさと fps から決める）、AAC | MP4 |
//! | `prores_422hq`（`prores`、`prores_ks`） | ProRes 422 HQ、10 ビット 4:2:2、24 ビット PCM | MOV |
//! | `prores_4444` | ProRes 4444、10 ビット 4:4:4、24 ビット PCM | MOV |
//! | `dnxhr_lb`、`dnxhr_sq`、`dnxhr_hq` | DNxHR（8 ビット 4:2:2）、24 ビット PCM | MOV |
//! | `dnxhr_hqx` | DNxHR HQX（10 ビット 4:2:2）、24 ビット PCM | MOV |
//! | `dnxhr_444` | DNxHR 444（10 ビット 4:4:4）、24 ビット PCM | MOV |
//!
//! 書く部分は `mm-encode-ffmpeg-core`。このクレートはコーデックの名前から指定を決める。
//! feature `ffmpeg` を入れたときだけ実装をビルドする（FFmpeg の開発ファイルが要るため）。
#![cfg(feature = "ffmpeg")]

mod codecs;

use mm_decode_api::{Encoder, EncoderFactory};
use mm_encode_ffmpeg_core::{encoder_available, init, FfmpegEncoder};
use mm_types::Result;

pub use codecs::{spec_of, SUPPORTED_NAMES};

/// 実装の名前。
pub const ID: &str = "ffmpeg";

/// FFmpeg のエンコーダーを作るインターフェース。
#[derive(Debug, Clone, Copy, Default)]
pub struct FfmpegEncoderFactory;

impl EncoderFactory for FfmpegEncoderFactory {
    fn id(&self) -> &'static str {
        ID
    }

    fn supports(&self, codec: &str) -> bool {
        spec_of(codec).is_some_and(|spec| init().is_ok() && encoder_available(spec.encoder))
    }

    fn create(&self) -> Result<Box<dyn Encoder>> {
        init()?;
        Ok(Box::new(FfmpegEncoder::new(spec_of)))
    }
}
