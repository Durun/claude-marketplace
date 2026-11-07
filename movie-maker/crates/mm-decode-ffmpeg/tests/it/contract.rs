//! `mm-testkit` のデコーダーの契約を、実素材で通す。

use crate::common;

use std::sync::Arc;

use mm_decode_api::DecoderFactory;
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_testkit::decode::{contract, DecodeFixture};
use mm_types::{MmError, Rational, RationalTime};

#[test]
fn ffmpeg_デコーダーが契約を通る() {
    let dir = common::fixtures();
    let fx = DecodeFixture {
        path: dir.join("cfr.mp4"),
        duration: RationalTime::seconds(2),
        fps: Rational::integer(30),
        broken_path: Some(dir.join("broken.mp4")),
    };
    contract(|| Arc::new(FfmpegDecoderFactory), &fx);
}

/// 伝達特性を PQ か HLG と宣言した素材（中身は単色）。`trc` は ffmpeg の `-color_trc` の名前。無ければ ffmpeg の CLI で作る。
fn hdr_transfer_fixture(trc: &str) -> std::path::PathBuf {
    let out = common::fixtures();
    let target = out.join(format!("trc_{trc}.mp4"));
    if target.exists() {
        return target;
    }
    let tmp = out.join(format!("trc_{trc}.{}.tmp.mp4", std::process::id()));
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg(format!(
            "color=c=0x606060:s=320x180:r=30,format=yuv420p,\
             setparams=color_primaries=bt2020:color_trc={trc}:colorspace=bt2020nc,trim=end_frame=10"
        ))
        .args(["-c:v", "libx264", "-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(&tmp)
        .status()
        .expect("ffmpeg の CLI を起動できた");
    assert!(status.success(), "{trc} の素材を作れた");
    if std::fs::rename(&tmp, &target).is_err() {
        // 別のプロセスが先に置いた
        let _ = std::fs::remove_file(&tmp);
    }
    target
}

/// PQ と HLG は Rec.1886 の式で線形にすると明るさが大きく狂うので、GPU の実装と同じ判定で拒否する。
#[test]
fn pq_と_hlg_の素材は拒否する() {
    for trc in ["smpte2084", "arib-std-b67"] {
        let path = hdr_transfer_fixture(trc);
        let opened = FfmpegDecoderFactory.open(&path, None);
        assert!(
            matches!(opened, Err(MmError::Unsupported(_))),
            "{trc}: {:?}",
            opened.err()
        );
    }
}

#[test]
fn 契約の素材でも焼き込んだ番号がフレームの番号と一致する() {
    let dir = common::fixtures();
    let mut dec = FfmpegDecoderFactory
        .open(&dir.join("cfr.mp4"), None)
        .unwrap();
    let frame = dec.next_frame().unwrap().unwrap();
    assert_eq!(common::read_frame_number(frame.image.as_ref().unwrap()), 0);
}
