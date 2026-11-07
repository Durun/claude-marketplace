//! ProRes、DNxHR、2 パスの x264 を書き、ffprobe とデコーダーで読み戻して、形式・フレーム数・番号・色・音を確かめる。

use crate::common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use mm_audio_api::AudioBlock;
use mm_decode_api::{AudioStreamInfo, DecoderFactory, EncodeSettings, EncoderFactory};
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_encode_ffmpeg::FfmpegEncoderFactory;
use mm_types::{MmError, Rational, Rgba, RgbaImage, Size};

const RATE: u32 = 48_000;
const FRAMES: i64 = 24;
/// 符号化値（0..255 換算）の許容ずれ。圧縮と 4:2:0・4:2:2 の色の縮みを含む。
const CODE_TOLERANCE: f32 = 3.0;

fn out_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("mm_encode_ffmpeg_codecs");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn numbered_frame(size: Size, number: i64) -> RgbaImage {
    let mut image = RgbaImage::filled(size, Rgba::new(0.37, 0.37, 0.37, 1.0));
    for k in 0..common::BIT_COUNT {
        let v = ((number >> k) & 1) as f32;
        for y in 0..common::BIT_SIZE {
            for x in 0..common::BIT_SIZE {
                image.set_pixel(k * common::BIT_SIZE + x, y, Rgba::new(v, v, v, 1.0));
            }
        }
    }
    image
}

fn probe(path: &Path, select: &str, entries: &str) -> HashMap<String, String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", select])
        .args(["-show_entries", entries, "-of", "default=nw=1"])
        .arg(path)
        .output()
        .expect("ffprobe を実行できる");
    assert!(
        out.status.success(),
        "ffprobe が読めた: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect()
}

fn settings(path: PathBuf, codec: &str, size: Size) -> EncodeSettings {
    EncodeSettings {
        size,
        fps: Rational::integer(24),
        codec: codec.into(),
        output_path: path,
        audio: Some(AudioStreamInfo {
            sample_rate: RATE,
            channels: 2,
        }),
    }
}

/// `passes` 回、同じ映像を送って閉じる。音は 1 秒の無音。
fn write(codec: &str, path: &Path, size: Size) {
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.to_path_buf(), codec, size))
        .unwrap();
    let frames: Vec<RgbaImage> = (0..FRAMES).map(|n| numbered_frame(size, n)).collect();
    for pass in 1..=enc.passes() {
        enc.begin_pass(pass).unwrap();
        for chunk in frames.chunks(5) {
            enc.push_frames(chunk).unwrap();
        }
    }
    enc.push_audio(&AudioBlock::silence(RATE, 2, RATE as usize))
        .unwrap();
    let report = enc.finish().unwrap();
    assert_eq!(
        report.frames, FRAMES as u64,
        "{codec}: 最後の 1 回の数だけ数える"
    );
    assert_eq!(report.audio_frames, u64::from(RATE));
}

fn check_numbers(path: &Path) {
    let mut dec = FfmpegDecoderFactory.open(path, None).unwrap();
    let mut n = 0;
    while let Some(f) = dec.next_frame().unwrap() {
        assert_eq!(common::read_frame_number(f.image.as_ref().unwrap()), n);
        n += 1;
    }
    assert_eq!(n, FRAMES);
}

#[test]
fn 各コーデックが形式とフレーム数と番号と音で読み戻せる() {
    let size = Size::new(320, 180);
    // (名前, 映像のコーデック, 画素形式, コンテナの拡張子)
    let cases = [
        ("prores_422hq", "prores", "yuv422p10le", "mov"),
        // ProRes 4444 のデコーダーは 12 ビットで返す
        ("prores_4444", "prores", "yuv444p12le", "mov"),
        ("dnxhr_hq", "dnxhd", "yuv422p", "mov"),
        ("dnxhr_hqx", "dnxhd", "yuv422p10le", "mov"),
        ("dnxhr_444", "dnxhd", "yuv444p10le", "mov"),
        ("h264_2pass", "h264", "yuv420p", "mp4"),
    ];
    for (codec, video, pix_fmt, ext) in cases {
        let path = out_path(&format!("{codec}.{ext}"));
        write(codec, &path, size);
        let v = probe(
            &path,
            "v:0",
            "stream=codec_name,pix_fmt,width,height,nb_read_frames,avg_frame_rate",
        );
        assert_eq!(v["codec_name"], video, "{codec}");
        assert_eq!(v["pix_fmt"], pix_fmt, "{codec}");
        assert_eq!(v["nb_read_frames"], FRAMES.to_string(), "{codec}");
        assert_eq!(v["avg_frame_rate"], "24/1", "{codec}");
        let a = probe(&path, "a:0", "stream=codec_name,sample_rate,channels");
        let want_audio = if ext == "mov" { "pcm_s24le" } else { "aac" };
        assert_eq!(a["codec_name"], want_audio, "{codec}");
        assert_eq!(a["sample_rate"], "48000", "{codec}");
        check_numbers(&path);
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn プロレスは_8_ビットを経由せず暗部の階調を保つ() {
    // 隣り合う 8 ビットの符号化値の間（線形で 0.0100 と 0.0103）は、8 ビットを経由すると同じ値になり、
    // 10 ビットで書けば別の値として読み戻せる。
    let size = Size::new(64, 32);
    let path = out_path("gradation.mov");
    let (a, b) = (
        Rgba::new(0.0100, 0.0100, 0.0100, 1.0),
        Rgba::new(0.0106, 0.0106, 0.0106, 1.0),
    );
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    let mut s = settings(path.clone(), "prores_4444", size);
    s.audio = None;
    enc.begin(&s).unwrap();
    enc.push_frames(&[RgbaImage::filled(size, a), RgbaImage::filled(size, b)])
        .unwrap();
    enc.finish().unwrap();
    let y = |f: u32| {
        let out = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&path)
            .args(["-vf", &format!("select=eq(n\\,{f})"), "-frames:v", "1"])
            .args(["-pix_fmt", "yuv444p10le", "-f", "rawvideo", "-"])
            .output()
            .expect("ffmpeg を実行できる");
        assert!(out.status.success());
        // Y プレーンの中央の 1 画素（10 ビットのリトルエンディアン）
        let i = (16 * 64 + 32) * 2;
        u16::from_le_bytes([out.stdout[i], out.stdout[i + 1]])
    };
    assert!(
        y(1) > y(0),
        "10 ビットなら 2 つの暗部の値が区別できる: {} {}",
        y(0),
        y(1)
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn 色は各コーデックで線形に戻る() {
    // デコーダーは行列が未指定のとき高さ 720 未満を BT.601 とみなす。ProRes は未指定で返すので HD の大きさで確かめる。
    let size = Size::new(1280, 720);
    let color = Rgba::new(0.02, 0.2, 0.6, 1.0);
    for codec in ["prores_422hq", "prores_4444", "dnxhr_hqx", "h264_2pass"] {
        let ext = if codec == "h264_2pass" { "mp4" } else { "mov" };
        let path = out_path(&format!("color_{codec}.{ext}"));
        let mut enc = FfmpegEncoderFactory.create().unwrap();
        let mut s = settings(path.clone(), codec, size);
        s.audio = None;
        enc.begin(&s).unwrap();
        for pass in 1..=enc.passes() {
            enc.begin_pass(pass).unwrap();
            enc.push_frames(&[RgbaImage::filled(size, color)]).unwrap();
        }
        enc.finish().unwrap();
        let mut dec = FfmpegDecoderFactory.open(&path, None).unwrap();
        let f = dec.next_frame().unwrap().expect("フレームがある");
        let p = f
            .image
            .as_ref()
            .unwrap()
            .pixel(size.width / 2, size.height / 2);
        for (got, want) in [(p.r, color.r), (p.g, color.g), (p.b, color.b)] {
            let code = |v: f32| v.max(0.0).powf(1.0 / 2.4) * 255.0;
            let diff = (code(got) - code(want)).abs();
            assert!(
                diff <= CODE_TOLERANCE,
                "{codec}: 線形 {want} が {got}（{diff} ステップ）"
            );
        }
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn 二パスの使い方の誤りはエラーになり統計ファイルを残さない() {
    let size = Size::new(64, 32);
    let path = out_path("misuse_2pass.mp4");
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.clone(), "h264_2pass", size))
        .unwrap();
    assert_eq!(enc.passes(), 2);
    assert!(
        matches!(
            enc.push_audio(&AudioBlock::silence(RATE, 2, 100)),
            Err(MmError::InvalidArgument(_))
        ),
        "1 回目に音は送れない"
    );
    assert!(enc.finish().is_err(), "2 回目を始める前には閉じられない");
    drop(enc);
    let stats = PathBuf::from(format!("{}.2pass.log", path.display()));
    assert!(!stats.exists(), "統計ファイルは消える");
    assert!(!PathBuf::from(format!("{}.mbtree", stats.display())).exists());
    assert!(!path.exists(), "1 回目は出力を作らない");
}

#[test]
fn nv12_を受けないコーデックは_unsupported_を返して_rgba_に戻らせる() {
    let size = Size::new(64, 32);
    let path = out_path("nv12_prores.mov");
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.clone(), "prores_422hq", size))
        .unwrap();
    assert!(matches!(
        enc.push_nv12_frames(&[]),
        Err(MmError::Unsupported(_))
    ));
    let _ = enc.finish();
    let _ = std::fs::remove_file(&path);

    let path = out_path("nv12_x264.mp4");
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.clone(), "h264", size)).unwrap();
    assert!(
        enc.push_nv12_frames(&[]).is_ok(),
        "libx264 は NV12 を受ける"
    );
    let _ = enc.finish();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn 実素材を_prores_と_dnxhr_と二パスに書き直しても番号が保たれる() {
    let source = common::fixtures().join("cfr.mp4");
    let mut dec = FfmpegDecoderFactory.open(&source, None).unwrap();
    let info = dec.info().clone();
    let fps = info.fps.unwrap();
    let frames: Vec<RgbaImage> = std::iter::from_fn(|| dec.next_frame().unwrap())
        .map(|f| f.image.unwrap())
        .collect();
    let cases = [
        ("prores_422hq", "mov"),
        ("dnxhr_hq", "mov"),
        ("h264_2pass", "mp4"),
    ];
    for (codec, ext) in cases {
        let path = out_path(&format!("reencode_{codec}.{ext}"));
        let mut enc = FfmpegEncoderFactory.create().unwrap();
        let mut s = settings(path.clone(), codec, info.size);
        s.fps = fps;
        s.audio = None;
        enc.begin(&s).unwrap();
        for pass in 1..=enc.passes() {
            enc.begin_pass(pass).unwrap();
            enc.push_frames(&frames).unwrap();
        }
        let report = enc.finish().unwrap();
        assert_eq!(report.frames, frames.len() as u64, "{codec}");
        let mut back = FfmpegDecoderFactory.open(&path, None).unwrap();
        assert_eq!(back.info().frame_count(), frames.len() as i64, "{codec}");
        let last = frames.len() as i64 - 1;
        for n in [0, last / 2, last] {
            back.seek(mm_types::RationalTime::from_frames(n, fps).unwrap())
                .unwrap();
            let f = back.next_frame().unwrap().unwrap();
            assert_eq!(
                common::read_frame_number(f.image.as_ref().unwrap()),
                n,
                "{codec}"
            );
        }
        let _ = std::fs::remove_file(&path);
    }
}
