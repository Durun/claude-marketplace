//! NVENC によるエンコーダー（`mm-decode-api` の `Encoder` の実装）。
//!
//! FFmpeg の `h264_nvenc` と `hevc_nvenc` を `mm-encode-ffmpeg-core` の上で使う。NVIDIA の SDK を直接使わないのは、
//! nvEncodeAPI の動的読み込み、セッションとバッファの管理、MP4 への多重化を FFmpeg が済ませていて、
//! 速さは同じエンコーダーのハードウェアで決まるため。NV12 は CPU のメモリから渡す（リードバックは 3 枚先行している）。
//!
//! NVIDIA の GPU かドライバーが無い環境では `supports` が偽を返し、組み立て（`mm-runtime`）が libx264 に落とす。
//!
//! | `EncodeSettings::codec` | 中身 | コンテナ |
//! |---|---|---|
//! | `h264_nvenc` | NVENC の H.264、VBR + CQ 19、AAC | MP4 |
//! | `hevc_nvenc` | NVENC の HEVC（`hvc1`）、VBR + CQ 21、AAC | MP4 |
//!
//! feature `ffmpeg` を入れたときだけ実装をビルドする。
#![cfg(feature = "ffmpeg")]

use std::sync::OnceLock;

use mm_decode_api::{Encoder, EncoderFactory};
use mm_encode_ffmpeg_core::{
    can_open, init, AudioCodec, CodecSpec, Container, FfmpegEncoder, Rate, VideoInput,
};
use mm_types::Result;

/// 実装の名前。
pub const ID: &str = "nvenc";

/// 扱う名前。
pub const SUPPORTED_NAMES: [&str; 2] = ["h264_nvenc", "hevc_nvenc"];

/// NVENC のプリセット。p1（最速）から p7（最高画質）の中間（p4）。RTX 5090 の 1 セッションは 1080p で p5 が約 360 fps、
/// p4 が約 500 fps で、1 プロセスの予算（120 fps）と並行の予算（300 fps）の両方に余裕を残せるのは p4。
const PRESET: &str = "p4";
/// H.264 と HEVC の定品質（CQ）。HEVC は同じ見た目で数値が大きい。
const H264_CQ: &str = "19";
const HEVC_CQ: &str = "21";

/// 名前から指定を決める。扱えない名前は `None`。
pub fn spec_of(codec: &str) -> Option<CodecSpec> {
    match codec {
        "h264_nvenc" => Some(nvenc("h264_nvenc", H264_CQ)),
        "hevc_nvenc" => Some(nvenc("hevc_nvenc", HEVC_CQ).with_codec_tag(*b"hvc1")),
        _ => None,
    }
}

fn nvenc(encoder: &'static str, cq: &'static str) -> CodecSpec {
    CodecSpec::new(
        encoder,
        Container::Mp4,
        VideoInput::Nv12,
        AudioCodec::Aac,
        Rate::Quality("cq", cq),
    )
    .option("preset", PRESET)
    .option("tune", "hq")
    .option("rc", "vbr")
}

/// この PC で NVENC が開けるか。最初の 1 回だけ、小さな大きさで実際に開いて確かめる。
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE
        .get_or_init(|| init().is_ok() && spec_of("h264_nvenc").is_some_and(|spec| can_open(&spec)))
}

/// NVENC のエンコーダーを作るインターフェース。
#[derive(Debug, Clone, Copy, Default)]
pub struct NvencEncoderFactory;

impl EncoderFactory for NvencEncoderFactory {
    fn id(&self) -> &'static str {
        ID
    }

    fn supports(&self, codec: &str) -> bool {
        spec_of(codec).is_some() && available()
    }

    fn create(&self) -> Result<Box<dyn Encoder>> {
        init()?;
        Ok(Box::new(FfmpegEncoder::new(spec_of)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名前は_nvenc_の二つだけ() {
        for name in SUPPORTED_NAMES {
            assert!(spec_of(name).is_some(), "{name}");
        }
        assert!(spec_of("h264").is_none(), "libx264 は mm-encode-ffmpeg");
    }

    #[test]
    fn hevc_は_quicktime_で開ける_tag_を付ける() {
        assert_eq!(spec_of("hevc_nvenc").unwrap().codec_tag, Some(*b"hvc1"));
        assert_eq!(spec_of("h264_nvenc").unwrap().codec_tag, None);
    }
}
