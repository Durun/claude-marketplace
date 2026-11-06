//! コーデックの名前から書き方（`CodecSpec`）を決める表。

use mm_encode_ffmpeg_core::{AudioCodec, CodecSpec, Container, Rate, VideoInput};

/// 扱う名前（エイリアスを含む）。
pub const SUPPORTED_NAMES: [&str; 13] = [
    "h264",
    "libx264",
    "h264_2pass",
    "prores",
    "prores_ks",
    "prores_422hq",
    "prores_4444",
    "dnxhr_lb",
    "dnxhr_sq",
    "dnxhr_hq",
    "dnxhr_hqx",
    "dnxhr_444",
    "dnxhd",
];

/// x264 の品質（小さいほど高画質）。編集の途中の確認と配信の両方に足りる値。
const X264_CRF: &str = "18";
const X264_PRESET: &str = "medium";
/// 2 パスの目標。1 画素 1 フレームあたりのビット数（1080p60 でおよそ 15 Mbps）。
const TWO_PASS_BITS_PER_PIXEL: f64 = 0.12;
/// ProRes の profile（prores_ks）。
const PRORES_HQ: &str = "hq";
const PRORES_4444: &str = "4444";
/// ProRes の vendor。QuickTime が Apple のエンコーダーとして扱う。
const PRORES_VENDOR: &str = "apl0";

/// 名前から指定を決める。扱えない名前は `None`。
pub fn spec_of(codec: &str) -> Option<CodecSpec> {
    match codec {
        "h264" | "libx264" => Some(x264()),
        "h264_2pass" => Some(
            CodecSpec::new(
                "libx264",
                Container::Mp4,
                VideoInput::Nv12,
                AudioCodec::Aac,
                Rate::BitsPerPixel(TWO_PASS_BITS_PER_PIXEL),
            )
            .option("preset", X264_PRESET)
            .with_passes(2),
        ),
        "prores" | "prores_ks" | "prores_422hq" => Some(prores(PRORES_HQ, VideoInput::Yuv422p10)),
        "prores_4444" => Some(prores(PRORES_4444, VideoInput::Yuv444p10)),
        "dnxhd" | "dnxhr_hq" => Some(dnxhr("dnxhr_hq", VideoInput::Yuv422p8)),
        "dnxhr_lb" => Some(dnxhr("dnxhr_lb", VideoInput::Yuv422p8)),
        "dnxhr_sq" => Some(dnxhr("dnxhr_sq", VideoInput::Yuv422p8)),
        "dnxhr_hqx" => Some(dnxhr("dnxhr_hqx", VideoInput::Yuv422p10)),
        "dnxhr_444" => Some(dnxhr("dnxhr_444", VideoInput::Yuv444p10)),
        _ => None,
    }
}

fn x264() -> CodecSpec {
    CodecSpec::new(
        "libx264",
        Container::Mp4,
        VideoInput::Nv12,
        AudioCodec::Aac,
        Rate::Quality("crf", X264_CRF),
    )
    .option("preset", X264_PRESET)
}

fn prores(profile: &str, input: VideoInput) -> CodecSpec {
    CodecSpec::new(
        "prores_ks",
        Container::Mov,
        input,
        AudioCodec::PcmS24,
        Rate::Profile,
    )
    .option("profile", profile)
    .option("vendor", PRORES_VENDOR)
}

fn dnxhr(profile: &str, input: VideoInput) -> CodecSpec {
    CodecSpec::new(
        "dnxhd",
        Container::Mov,
        input,
        AudioCodec::PcmS24,
        Rate::Profile,
    )
    .option("profile", profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 扱う名前はすべて指定になる() {
        for name in SUPPORTED_NAMES {
            assert!(spec_of(name).is_some(), "{name}");
        }
        assert!(spec_of("h264_nvenc").is_none(), "NVENC は別のクレート");
        assert!(spec_of("vp9").is_none());
    }

    #[test]
    fn コンテナと画素形式はコーデックで決まる() {
        let x = spec_of("h264").unwrap();
        assert_eq!(
            (x.container, x.input, x.passes),
            (Container::Mp4, VideoInput::Nv12, 1)
        );
        let two = spec_of("h264_2pass").unwrap();
        assert_eq!(two.passes, 2);
        assert!(
            matches!(two.rate, Rate::BitsPerPixel(_)),
            "2 パスは目標のビットレート"
        );
        let p = spec_of("prores_4444").unwrap();
        assert_eq!(
            (p.container, p.input),
            (Container::Mov, VideoInput::Yuv444p10)
        );
        let d = spec_of("dnxhr_hqx").unwrap();
        assert_eq!((d.encoder, d.input), ("dnxhd", VideoInput::Yuv422p10));
        assert_eq!(spec_of("dnxhr_hq").unwrap().input, VideoInput::Yuv422p8);
    }
}
