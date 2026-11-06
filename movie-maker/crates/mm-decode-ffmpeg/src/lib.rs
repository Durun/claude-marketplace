//! FFmpeg のライブラリによるデコーダー（`mm-decode-api` の実装）。
//!
//! - フレームの番号は、開くときに読む映像パケットの PTS の索引の位置で決める。B フレーム、
//!   シーク後、VFR でも同じ素材は同じ番号になる。
//! - 素材の時間軸の 0 は最初のフレームの PTS。コンテナの開始時刻が 0 でなくてもフレーム 0 が t = 0。
//! - 固定フレームレート（CFR）なら `fps` を返し、フレームの `pts` は `index / fps`。
//!   そうでなければ VFR として `pts_index` に各フレームの時刻を返す。
//! - 壊れた素材は `Err(Corrupt)`。
//!
//! feature `ffmpeg` を入れたときだけ実装をビルドする（FFmpeg の開発ファイルが要るため）。
#![cfg(feature = "ffmpeg")]

mod audio;
mod convert;
mod transfer;
#[cfg(feature = "gpu")]
mod upload;
mod video;

use std::path::Path;
use std::sync::OnceLock;

use mm_decode_api::{Decoder, DecoderFactory, GpuContextRef};
use mm_types::{MmError, Result};

pub use video::FfmpegDecoder;

/// 実装の名前。`mm-runtime` の設定で選ぶ。
pub const ID: &str = "ffmpeg";

/// FFmpeg を開くデコーダーのインターフェース。
#[derive(Debug, Clone, Copy, Default)]
pub struct FfmpegDecoderFactory;

impl DecoderFactory for FfmpegDecoderFactory {
    fn id(&self) -> &'static str {
        ID
    }

    /// デコードは CPU。feature `gpu` で `mm-gpu` のコンテキストを渡されたときは、YUV のプレーンを上げて
    /// GPU のフレームで返す。それ以外は CPU の RGBA。
    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        init()?;
        #[cfg(feature = "gpu")]
        if let Some(ctx) = gpu.and_then(|g| g.as_any().downcast_ref::<mm_gpu::GpuContext>()) {
            return Ok(Box::new(FfmpegDecoder::open_with_gpu(path, ctx)?));
        }
        let _ = gpu;
        Ok(Box::new(FfmpegDecoder::open(path)?))
    }
}

/// GPU のコンテキストを渡されても CPU の RGBA で返すインターフェース。経路の比較の計測と、GPU に上げない確かめに使う。
#[derive(Debug, Clone, Copy, Default)]
pub struct FfmpegCpuDecoderFactory;

impl DecoderFactory for FfmpegCpuDecoderFactory {
    fn id(&self) -> &'static str {
        "ffmpeg_cpu"
    }

    fn open(&self, path: &Path, _gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        init()?;
        Ok(Box::new(FfmpegDecoder::open(path)?))
    }
}

/// FFmpeg の初期化を 1 回だけ行う。
pub fn init() -> Result<()> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        ffmpeg_next::init().map_err(|e| e.to_string())?;
        ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);
        Ok(())
    })
    .clone()
    .map_err(|e| MmError::Internal(format!("FFmpeg の初期化: {e}")))
}
