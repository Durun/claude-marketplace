//! 見本のプロジェクトを `mm render` で書き出し、ffprobe と mm-decode-ffmpeg で読み戻して確かめる。
//! FFmpeg が要るので feature `ffmpeg` のときだけ走る。
#![cfg(feature = "ffmpeg")]

#[path = "../../../crates/mm-decode-ffmpeg/tests/common/mod.rs"]
mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use mm_decode_api::DecoderFactory;
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_types::{RationalTime, TimeRange};

/// 素材を in 点（0.5 秒）で切るので、書き出しの最初のコマは素材の 15 コマ目。
const FIRST_SOURCE_FRAME: i64 = 15;
const FRAMES: i64 = 30;

fn project_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/stage0/project.json")
}

/// 素材を用意してから `mm render` を呼び、出力先を返す。
fn render_stage0(name: &str) -> PathBuf {
    common::fixtures();
    let dir = std::env::temp_dir().join(format!("mm_cli_render_{}", std::process::id()));
    let out = dir.join(name);
    let output = Command::new(env!("CARGO_BIN_EXE_mm"))
        .arg("render")
        .arg(project_path())
        .arg("-o")
        .arg(&out)
        .output()
        .expect("mm を実行できる");
    assert!(
        output.status.success(),
        "mm render が成功する: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

/// ffprobe の `key=value` 出力を引く。
fn probe(path: &Path, select: &str, entries: &str) -> HashMap<String, String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", select])
        .args(["-show_entries", entries, "-of", "default=nw=1"])
        .arg(path)
        .output()
        .expect("ffprobe を実行できる");
    assert!(out.status.success(), "ffprobe が読めた");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect()
}

#[test]
fn 書き出した_mp4_のコマ数と長さとコーデックが合う() {
    let path = render_stage0("probe.mp4");
    let v = probe(
        &path,
        "v:0",
        "stream=codec_name,width,height,nb_read_frames,avg_frame_rate,duration",
    );
    assert_eq!(v["codec_name"], "h264");
    assert_eq!((v["width"].as_str(), v["height"].as_str()), ("320", "180"));
    assert_eq!(v["nb_read_frames"], FRAMES.to_string());
    assert_eq!(v["avg_frame_rate"], "30/1");
    let seconds: f64 = v["duration"].parse().expect("長さが数");
    assert!((seconds - 1.0).abs() < 0.05, "映像の長さ {seconds}");

    let a = probe(
        &path,
        "a:0",
        "stream=codec_name,sample_rate,channels,duration",
    );
    assert_eq!(a["codec_name"], "aac");
    assert_eq!(a["sample_rate"], "48000");
    let audio_seconds: f64 = a["duration"].parse().expect("音の長さが数");
    assert!(
        (audio_seconds - 1.0).abs() < 0.1,
        "音の長さ {audio_seconds}"
    );
}

#[test]
fn 切った区間のコマ番号と音が読み戻せる() {
    let path = render_stage0("readback.mp4");
    let mut decoder = FfmpegDecoderFactory.open(&path, None).expect("開ける");
    let info = decoder.info().clone();
    assert_eq!(info.frame_count(), FRAMES);

    decoder.seek(RationalTime::ZERO).expect("先頭へ");
    for n in 0..FRAMES {
        let frame = decoder.next_frame().expect("読める").expect("コマがある");
        let image = frame.image.expect("CPU の絵");
        assert_eq!(
            common::read_frame_number(&image),
            FIRST_SOURCE_FRAME + n,
            "書き出しの {n} コマ目の素材の番号"
        );
    }
    assert!(decoder.next_frame().expect("読める").is_none());

    let range =
        TimeRange::from_start_duration(RationalTime::ZERO, RationalTime::seconds(1)).expect("区間");
    let audio = decoder.decode_audio(range).expect("音が読める");
    assert!(!audio.is_silent(), "トーンが乗っている");
}

fn mm_ok(args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_mm"))
        .args(args)
        .output()
        .expect("mm を実行できる");
    assert!(
        out.status.success(),
        "mm {args:?} が失敗した: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 段階 0 の JSON を `mm import` で `project.mmdb` にし、`mm op` でレイヤーを動かして `mm render` で書き出す。
#[test]
fn 取り込んだ_mmdb_のレイヤーを動かして書き出せる() {
    common::fixtures();
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let dir_s = dir.to_str().unwrap();
    mm_ok(&["import", dir_s, project_path().to_str().unwrap()]);

    let before = tmp.path().join("before.mp4");
    mm_ok(&["render", dir_s, "-o", before.to_str().unwrap()]);
    mm_ok(&[
        "op",
        dir_s,
        "setParam",
        "--target",
        "layer:l-text:transform.position",
        "--json",
        r#"{"value":{"type":"vec2","value":[160.0,20.0]}}"#,
    ]);
    let after = tmp.path().join("after.mp4");
    mm_ok(&["render", dir_s, "-o", after.to_str().unwrap()]);

    let (b, a) = (
        std::fs::read(&before).unwrap(),
        std::fs::read(&after).unwrap(),
    );
    assert!(!b.is_empty() && !a.is_empty());
    assert_ne!(b, a, "レイヤーを動かした書き出しが変わっていない");
}
