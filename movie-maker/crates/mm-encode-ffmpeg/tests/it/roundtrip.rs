//! 書き出した MP4 を ffprobe で読み、デコーダーでリードバックして、フレーム数・長さ・番号・音を確かめる。

use crate::common;

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Command;

use mm_audio_api::AudioBlock;
use mm_decode_api::{AudioStreamInfo, DecoderFactory, EncodeSettings, EncoderFactory};
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_encode_ffmpeg::FfmpegEncoderFactory;
use mm_types::{MmError, Rational, RationalTime, Rgba, RgbaImage, Size, TimeRange};

const RATE: u32 = 48_000;

fn out_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("mm_encode_ffmpeg_roundtrip");
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

fn tone(start: usize, frames: usize) -> AudioBlock {
    let mut samples = Vec::with_capacity(frames * 2);
    for i in start..start + frames {
        let v = 0.25 * (TAU * 440.0 * i as f32 / RATE as f32).sin();
        samples.extend([v, v]);
    }
    AudioBlock::from_samples(RATE, 2, samples).unwrap()
}

/// ffprobe の `key=value` 出力を引く。
fn probe(path: &Path, select: &str, entries: &str) -> HashMap<String, String> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            select,
            "-show_entries",
        ])
        .arg(entries)
        .args(["-of", "default=nw=1"])
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

fn seconds(map: &HashMap<String, String>, key: &str) -> f64 {
    map[key]
        .parse()
        .unwrap_or_else(|_| panic!("{key} が数: {}", map[key]))
}

fn settings(path: PathBuf, size: Size, fps: Rational, audio: bool) -> EncodeSettings {
    EncodeSettings {
        size,
        fps,
        codec: "h264".into(),
        output_path: path,
        audio: audio.then_some(AudioStreamInfo {
            sample_rate: RATE,
            channels: 2,
        }),
    }
}

#[test]
fn 書き出した_mp4_がフレーム数と長さと番号と音で読み戻せる() {
    let path = out_path("tone.mp4");
    let size = Size::new(320, 180);
    let fps = Rational::integer(30);
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.clone(), size, fps, true)).unwrap();
    let frames: Vec<RgbaImage> = (0..60).map(|n| numbered_frame(size, n)).collect();
    for chunk in frames.chunks(7) {
        enc.push_frames(chunk).unwrap();
    }
    // AAC のフレーム（1024）とそろわない刻みで 2 秒分
    let mut sent = 0;
    for len in [1_000usize, 5_003, 40_000, 49_997] {
        enc.push_audio(&tone(sent, len)).unwrap();
        sent += len;
    }
    assert_eq!(sent, 96_000);
    let report = enc.finish().unwrap();
    assert_eq!((report.frames, report.audio_frames), (60, 96_000));
    assert_eq!(
        report.bytes_written,
        std::fs::metadata(&path).unwrap().len()
    );

    let v = probe(
        &path,
        "v:0",
        "stream=codec_name,width,height,nb_read_frames,avg_frame_rate,duration",
    );
    assert_eq!(v["codec_name"], "h264");
    assert_eq!((v["width"].as_str(), v["height"].as_str()), ("320", "180"));
    assert_eq!(v["nb_read_frames"], "60");
    assert_eq!(v["avg_frame_rate"], "30/1");
    assert!(
        (seconds(&v, "duration") - 2.0).abs() < 0.05,
        "映像の長さ {}",
        v["duration"]
    );
    let a = probe(
        &path,
        "a:0",
        "stream=codec_name,sample_rate,channels,duration",
    );
    assert_eq!(a["codec_name"], "aac");
    assert_eq!(
        (a["sample_rate"].as_str(), a["channels"].as_str()),
        ("48000", "2")
    );
    assert!(
        (seconds(&a, "duration") - 2.0).abs() < 0.1,
        "音の長さ {}",
        a["duration"]
    );

    let mut dec = FfmpegDecoderFactory.open(&path, None).unwrap();
    assert_eq!(dec.info().fps, Some(fps));
    assert_eq!(dec.info().frame_count(), 60);
    let mut n = 0;
    while let Some(f) = dec.next_frame().unwrap() {
        assert_eq!(
            common::read_frame_number(f.image.as_ref().unwrap()),
            n,
            "フレーム {n}"
        );
        n += 1;
    }
    assert_eq!(n, 60);

    // 音: 0.5〜0.6 秒に 440Hz が約 44 周期。振幅は入れた 0.25 に近い。
    let range = TimeRange::new(
        RationalTime::new(1, 2).unwrap(),
        RationalTime::new(6, 10).unwrap(),
    )
    .unwrap();
    let block = dec.decode_audio(range).unwrap();
    assert_eq!(block.frames(), 4_800);
    let peak = block.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((0.2..0.3).contains(&peak), "振幅 {peak}");
    let left: Vec<f32> = block.samples.iter().step_by(2).copied().collect();
    let crossings = left
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    assert!((42..=46).contains(&crossings), "周期の数 {crossings}");
}

#[test]
fn 分数の_fps_でもフレーム数と時刻が合う() {
    let path = out_path("ntsc.mp4");
    let size = Size::new(256, 64);
    let fps = Rational::new(30_000, 1001).unwrap();
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.clone(), size, fps, false))
        .unwrap();
    let frames: Vec<RgbaImage> = (0..30).map(|n| numbered_frame(size, n)).collect();
    enc.push_frames(&frames).unwrap();
    assert_eq!(enc.finish().unwrap().frames, 30);

    let v = probe(&path, "v:0", "stream=nb_read_frames,avg_frame_rate");
    assert_eq!(v["nb_read_frames"], "30");
    assert_eq!(v["avg_frame_rate"], "30000/1001");
    let mut dec = FfmpegDecoderFactory.open(&path, None).unwrap();
    assert_eq!(dec.info().fps, Some(fps));
    for n in 0..30 {
        let f = dec.next_frame().unwrap().expect("フレームがある");
        assert_eq!(f.pts, RationalTime::from_frames(n, fps).unwrap());
        assert_eq!(common::read_frame_number(f.image.as_ref().unwrap()), n);
    }
}

#[test]
fn 実素材を読んで書き直しても番号が保たれる() {
    let source = common::fixtures().join("cfr.mp4");
    let mut dec = FfmpegDecoderFactory.open(&source, None).unwrap();
    let info = dec.info().clone();
    let fps = info.fps.unwrap();
    let path = out_path("reencode.mp4");
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.clone(), info.size, fps, true))
        .unwrap();
    while let Some(f) = dec.next_frame().unwrap() {
        enc.push_frames(&[f.image.unwrap()]).unwrap();
    }
    let whole = TimeRange::new(RationalTime::ZERO, info.duration).unwrap();
    enc.push_audio(&dec.decode_audio(whole).unwrap()).unwrap();
    let report = enc.finish().unwrap();
    assert_eq!((report.frames, report.audio_frames), (60, 96_000));

    let mut back = FfmpegDecoderFactory.open(&path, None).unwrap();
    assert_eq!(back.info().frame_count(), 60);
    for n in [0, 29, 59] {
        back.seek(RationalTime::from_frames(n, fps).unwrap())
            .unwrap();
        let f = back.next_frame().unwrap().unwrap();
        assert_eq!(common::read_frame_number(f.image.as_ref().unwrap()), n);
    }
}

#[test]
fn 使い方の誤りは_panic_せずエラーになる() {
    let size = Size::new(32, 16);
    let fps = Rational::integer(30);

    let mut enc = FfmpegEncoderFactory.create().unwrap();
    let odd = settings(out_path("odd.mp4"), Size::new(33, 16), fps, false);
    assert!(matches!(enc.begin(&odd), Err(MmError::InvalidArgument(_))));

    let mut enc = FfmpegEncoderFactory.create().unwrap();
    let mut unsupported = settings(out_path("vp9.webm"), size, fps, false);
    unsupported.codec = "vp9".into();
    assert!(matches!(
        enc.begin(&unsupported),
        Err(MmError::Unsupported(_))
    ));

    let mut enc = FfmpegEncoderFactory.create().unwrap();
    assert!(enc.finish().is_err(), "begin の前の finish");
    enc.begin(&settings(out_path("misuse.mp4"), size, fps, false))
        .unwrap();
    assert!(
        enc.push_frames(&[numbered_frame(Size::new(64, 16), 0)])
            .is_err(),
        "大きさ違い"
    );
    assert!(enc.push_audio(&tone(0, 1024)).is_err(), "設定に音が無い");
    enc.push_frames(&[numbered_frame(size, 0)]).unwrap();
    enc.finish().unwrap();
    assert!(
        enc.push_frames(&[numbered_frame(size, 0)]).is_err(),
        "finish の後"
    );
}

/// 線形の値 -> エンコード -> デコードで、符号化値の 1/255 を単位に何ステップずれるかの上限。
/// 4:2:0 の間引きと tv レンジの量子化、H.264 の圧縮（crf 18）の分。
const ROUNDTRIP_CODE_TOLERANCE: f32 = 3.0;

#[test]
fn 線形の色を書き出してリードバックしても明るさがずれない() {
    let size = Size::new(64, 32);
    let fps = Rational::integer(30);
    let path = out_path("color.mp4");
    // 暗部、中間、明部。チャンネルごとに別の値にして、取り違えも拾う
    let colors = [
        Rgba::new(0.02, 0.2, 0.6, 1.0),
        Rgba::new(0.6, 0.02, 0.2, 1.0),
        Rgba::new(0.2, 0.2, 0.2, 1.0),
        Rgba::new(0.9, 0.9, 0.9, 1.0),
    ];
    let mut enc = FfmpegEncoderFactory.create().unwrap();
    enc.begin(&settings(path.clone(), size, fps, false))
        .unwrap();
    for c in colors {
        enc.push_frames(&[RgbaImage::filled(size, c)]).unwrap();
    }
    enc.finish().unwrap();

    let mut dec = FfmpegDecoderFactory.open(&path, None).unwrap();
    for c in colors {
        let f = dec.next_frame().unwrap().expect("フレームがある");
        let p = f
            .image
            .as_ref()
            .unwrap()
            .pixel(size.width / 2, size.height / 2);
        for (got, want) in [(p.r, c.r), (p.g, c.g), (p.b, c.b)] {
            let code = |v: f32| v.max(0.0).powf(1.0 / 2.4) * 255.0;
            let diff = (code(got) - code(want)).abs();
            assert!(
                diff <= ROUNDTRIP_CODE_TOLERANCE,
                "線形 {want} が {got} に戻った（符号化値で {diff} ステップのずれ）"
            );
        }
    }
}
