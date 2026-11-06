//! NVENC で書いた MP4 を ffprobe で読み、形式とフレーム数を確かめる。NVIDIA の GPU が無い環境では、使えないと返すことだけを確かめる。
#![cfg(feature = "ffmpeg")]

use std::path::{Path, PathBuf};
use std::process::Command;

use mm_decode_api::{EncodeSettings, EncoderFactory, Nv12Frame};
use mm_encode_nvenc::{available, NvencEncoderFactory};
use mm_types::{Rational, Rgba, RgbaImage, Size};

const FRAMES: usize = 30;

fn out_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("mm_encode_nvenc");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn settings(path: PathBuf, codec: &str, size: Size) -> EncodeSettings {
    EncodeSettings {
        size,
        fps: Rational::integer(30),
        codec: codec.into(),
        output_path: path,
        audio: None,
    }
}

/// 明るさが 1 フレームごとに変わる NV12。
fn nv12(size: Size, n: usize) -> Nv12Frame {
    let (w, h) = (size.width as usize, size.height as usize);
    let luma = 40 + (n * 6) as u8;
    Nv12Frame::new(size, vec![luma; w * h], vec![128; w * h / 2]).unwrap()
}

fn probe(path: &Path, entries: &str) -> String {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
        .args(["-show_entries", entries, "-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("ffprobe を実行できる");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn nvenc_が無い環境では_supports_が偽で_あれば二つの名前を扱う() {
    let f = NvencEncoderFactory;
    assert!(!f.supports("h264"), "libx264 は別の実装");
    assert!(!f.supports("prores_422hq"));
    assert_eq!(f.supports("h264_nvenc"), available());
    assert_eq!(f.supports("hevc_nvenc"), available());
}

#[test]
fn nv12_と_rgba_のどちらでも書けて_ffprobe_で読める() {
    if !available() {
        eprintln!("NVENC が使えない環境なので飛ばす");
        return;
    }
    let size = Size::new(1280, 720);
    for (codec, name, tag) in [
        ("h264_nvenc", "h264", "avc1"),
        ("hevc_nvenc", "hevc", "hvc1"),
    ] {
        let path = out_path(&format!("{codec}.mp4"));
        let mut enc = NvencEncoderFactory.create().unwrap();
        enc.begin(&settings(path.clone(), codec, size)).unwrap();
        assert!(enc.push_nv12_frames(&[]).is_ok(), "NV12 を受ける");
        let frames: Vec<Nv12Frame> = (0..FRAMES / 2).map(|n| nv12(size, n)).collect();
        for chunk in frames.chunks(4) {
            enc.push_nv12_frames(chunk).unwrap();
        }
        let rgba: Vec<RgbaImage> = (0..FRAMES / 2)
            .map(|_| RgbaImage::filled(size, Rgba::new(0.2, 0.2, 0.2, 1.0)))
            .collect();
        enc.push_frames(&rgba).unwrap();
        let report = enc.finish().unwrap();
        assert_eq!(report.frames, FRAMES as u64);
        assert_eq!(
            probe(
                &path,
                "stream=codec_name,codec_tag_string,nb_read_frames,width,height"
            ),
            format!("{name},{tag},1280,720,{FRAMES}"),
            "{codec}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
