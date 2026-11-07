//! 字幕の焼き込みの通し。素材 1 本と合成の文字起こし（0.5 秒ごとの単語「語N」）から sequence を作り、焼き込む設定と
//! 焼き込まない設定の 2 つのレンダーターゲットで `mm render` する。確かめること:
//! - 焼き込んだ MP4 のフレームは、字幕（書き出しの隣の SRT）の出ている間だけ、焼き込まない MP4 と違う。
//! - 違う画素は下部にある（既定は下部中央）。
//!
//! `MM_BURN_IN_FRAMES` にフォルダーを渡すと、確かめたフレームを PNG で置く（目で見る用）。
//! FFmpeg が要るので feature `ffmpeg` のときだけ実行する。

use std::path::Path;
use std::process::{Command, Output};

use mm_store::Store;
use mm_types::RationalTime;
use serde_json::{json, Value};

const MEDIA_SECONDS: u32 = 12;
const WORDS: i64 = 24;
const WORDS_PER_SENTENCE: i64 = 6;
/// 話していない区間（字幕が出ない時間）の単語の番号。
const SILENT_WORDS: std::ops::Range<i64> = 12..16;
/// 書き出しのフレームレート。単語の境界（0.25 秒の倍数）がフレームの境界に乗る最小の値。CI の GPU は WARP
/// （ソフトウェア）でフレームごとに重いので低くする。
const FPS: u32 = 4;
/// 書き出しのフレームの大きさ。字幕の字の大きさは短い辺に比例するので、見える大きさにする。
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
/// 字幕の境界から離して見るフレーム数（0.25 秒）。ms への丸めと MP4 の時刻の誤差に影響されない余白。
const MARGIN_FRAMES: i64 = 1;
/// 字幕が出ているとみなす、違う画素の数と 1 画素の輝度の差（0〜255）。
const MIN_DIFF_PIXELS: usize = 200;
const PIXEL_DIFF: i32 = 60;
/// 確かめる字幕の件数。
const CHECKED_CUES: usize = 3;

fn run(cmd: &mut Command) -> Output {
    let out = cmd.output().expect("コマンドを実行できる");
    assert!(
        out.status.success(),
        "{cmd:?} が失敗した: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn mm(args: &[&str]) -> Output {
    run(Command::new(env!("CARGO_BIN_EXE_mm")).args(args))
}

fn op(dir: &str, value: Value) {
    let name = value["op"].as_str().unwrap().to_owned();
    let mut args = value;
    args.as_object_mut().unwrap().remove("op");
    mm(&["op", dir, &name, "--json", &args.to_string()]);
}

fn rt(num: i64, den: u32) -> Value {
    json!(RationalTime::new(i128::from(num), den).unwrap())
}

fn make_media(out: &Path) {
    let seconds = MEDIA_SECONDS.to_string();
    run(Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=320x180:r=30:d={seconds}"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!(
            "sine=frequency=440:sample_rate=48000:duration={seconds}"
        ))
        .args(["-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(out));
}

/// 合成の文字起こしを、素材のコンテンツハッシュの付属ファイルとして置く。
fn place_transcript(dir: &Path, media: &Path) {
    let hash = mm_ingest::hash_file(media).unwrap();
    let segments: Vec<Value> = (0..WORDS / WORDS_PER_SENTENCE)
        .map(|s| {
            let range = s * WORDS_PER_SENTENCE..(s + 1) * WORDS_PER_SENTENCE;
            let words: Vec<Value> = range
                .clone()
                .filter(|i| !SILENT_WORDS.contains(i))
                .map(|i| {
                    let mark = if i % WORDS_PER_SENTENCE == WORDS_PER_SENTENCE - 1 {
                        "。"
                    } else {
                        ""
                    };
                    json!({"start": rt(i, 2), "end": rt(2 * i + 1, 4), "text": format!("字幕の語{i}{mark}")})
                })
                .collect();
            json!({"start": rt(range.start, 2), "end": rt(2 * (range.end - 1) + 1, 4),
                "text": "", "words": words})
        })
        .collect();
    let sidecar = json!({"schema_version": 1, "content_hash": hash, "producer": "test",
        "kind": "transcript", "language": "ja", "segments": segments});
    let path = mm_ingest::CacheLayout::new(dir.join("cache")).sidecar(&hash, "transcript");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, sidecar.to_string()).unwrap();
}

/// 素材の全部を 1 つの sequence にし、それを置いたコンポジションと、焼き込む・焼き込まないの 2 つの出力を作る。
fn build_project(dir: &str, asset: &str) {
    let footage = json!({"kind": "footage", "version": 1,
        "params": {"asset": {"value": {"type": "asset_ref", "value": asset}}}});
    op(
        dir,
        json!({"op": "addSequence", "sequence": {"id": "s1", "cuts": [
            {"id": "k1", "source": footage, "in": rt(0, 1), "out": rt(i64::from(MEDIA_SECONDS), 1)}]}}),
    );
    let source = json!({"kind": "sequence", "version": 1,
        "params": {"sequence": {"value": {"type": "text", "value": "s1"}}}});
    let seconds = i64::from(MEDIA_SECONDS);
    let layer = json!({"id": "l1", "source": source, "start": rt(0, 1), "in": rt(0, 1),
        "out": rt(seconds, 1), "opacity": {"value": {"type": "number", "value": 1.0}}});
    op(
        dir,
        json!({"op": "addComposition", "composition": {"id": "main", "width": WIDTH,
        "height": HEIGHT, "fps": {"num": FPS, "den": 1}, "duration": rt(seconds, 1),
        "layers": [layer]}}),
    );
    for (id, delivery) in [
        (
            "burned",
            json!({"subtitles": "sidecar", "burn_in_captions": true}),
        ),
        ("plain", json!({})),
    ] {
        op(
            dir,
            json!({"op": "addRenderTarget", "target": {"id": id, "composition": "main",
            "width": WIDTH, "height": HEIGHT, "fps": {"num": FPS, "den": 1}, "codec": "h264",
            "output_path": format!("{id}.mp4"), "delivery": delivery}}),
        );
    }
}

/// 字幕 1 件の時刻（秒）。
struct Cue {
    start: f64,
    end: f64,
}

fn parse_time(s: &str) -> f64 {
    let (hms, ms) = s.trim().split_once([',', '.']).unwrap();
    let p: Vec<f64> = hms.split(':').map(|v| v.parse().unwrap()).collect();
    p[0] * 3600.0 + p[1] * 60.0 + p[2] + ms.parse::<f64>().unwrap() / 1000.0
}

fn parse_srt(text: &str) -> Vec<Cue> {
    text.replace("\r\n", "\n")
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .map(|block| {
            let (a, b) = block.lines().nth(1).unwrap().split_once("-->").unwrap();
            Cue {
                start: parse_time(a),
                end: parse_time(b),
            }
        })
        .collect()
}

/// MP4 のフレーム `index` を、輝度（0〜255）の並びで返す。`png` があればその名前でも置く。
fn luma(video: &Path, index: i64, png: Option<&Path>) -> Vec<u8> {
    let select = format!("select=eq(n\\,{index})");
    if let Some(png) = png {
        run(Command::new("ffmpeg")
            .args(["-hide_banner", "-v", "error", "-y", "-i"])
            .arg(video)
            .args(["-vf", &select, "-fps_mode", "passthrough", "-frames:v", "1"])
            .arg(png));
    }
    run(Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-i"])
        .arg(video)
        .args([
            "-vf",
            &format!("{select},format=gray"),
            "-fps_mode",
            "passthrough",
        ])
        .args(["-frames:v", "1", "-f", "rawvideo", "-"]))
    .stdout
}

/// 2 枚の違う画素の数と、その縦の範囲。
fn diff(a: &[u8], b: &[u8]) -> (usize, u32, u32) {
    let (mut count, mut top, mut bottom) = (0, u32::MAX, 0);
    for (i, (p, q)) in a.iter().zip(b).enumerate() {
        if (i32::from(*p) - i32::from(*q)).abs() > PIXEL_DIFF {
            let y = (i / WIDTH as usize) as u32;
            (count, top, bottom) = (count + 1, top.min(y), bottom.max(y));
        }
    }
    (count, top, bottom)
}

/// 字幕の外側のフレーム（番号）。字幕の前後の余白の分だけ離れていて、ほかの字幕に重ならない。
fn frames_to_check(cues: &[Cue]) -> Vec<(i64, bool)> {
    let at = |s: f64| (s * f64::from(FPS)).round() as i64;
    let covered = |f: i64| {
        cues.iter()
            .any(|c| at(c.start) - MARGIN_FRAMES <= f && f < at(c.end) + MARGIN_FRAMES)
    };
    let mut out = Vec::new();
    for c in cues.iter().take(CHECKED_CUES) {
        let (start, end) = (at(c.start), at(c.end));
        out.push(((start + end) / 2, true));
        out.push((start + MARGIN_FRAMES, true));
        out.push((end - MARGIN_FRAMES, true));
        for f in [start - MARGIN_FRAMES * 2, end + MARGIN_FRAMES * 2] {
            if f >= 0 && !covered(f) {
                out.push((f, false));
            }
        }
    }
    out
}

#[test]
fn 焼き込んだ字幕は字幕の時間だけ下部に出る() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let media = tmp.path().join("talk.mp4");
    make_media(&media);
    let d = dir.to_str().unwrap();
    mm(&["new", d]);
    place_transcript(&dir, &media);
    mm(&["ingest", d, media.to_str().unwrap(), "--encoder", "x264"]);
    let asset = Store::open_read_only(&dir).unwrap().project().assets[0]
        .id
        .clone();
    build_project(d, asset.as_str());

    let (burned, plain) = (tmp.path().join("burned.mp4"), tmp.path().join("plain.mp4"));
    mm(&[
        "render",
        d,
        "--target",
        "burned",
        "-o",
        burned.to_str().unwrap(),
    ]);
    mm(&[
        "render",
        d,
        "--target",
        "plain",
        "-o",
        plain.to_str().unwrap(),
    ]);
    let cues = parse_srt(&std::fs::read_to_string(burned.with_extension("srt")).unwrap());
    assert!(cues.len() > CHECKED_CUES, "字幕が少ない: {}", cues.len());

    let keep = std::env::var_os("MM_BURN_IN_FRAMES").map(std::path::PathBuf::from);
    if let Some(keep) = &keep {
        std::fs::create_dir_all(keep).unwrap();
    }
    let frames = frames_to_check(&cues);
    assert!(
        frames.iter().any(|(_, expected)| !expected),
        "字幕の出ていないフレームを確かめていない: {frames:?}"
    );
    for (index, expected) in frames {
        let png = keep.as_ref().map(|k| {
            k.join(format!(
                "frame-{index:04}-{}.png",
                if expected { "on" } else { "off" }
            ))
        });
        let (count, top, bottom) = diff(
            &luma(&burned, index, png.as_deref()),
            &luma(&plain, index, None),
        );
        let shown = count >= MIN_DIFF_PIXELS;
        assert_eq!(
            shown, expected,
            "フレーム {index} の字幕の有無（違う画素 {count}）"
        );
        if shown {
            assert!(top > HEIGHT / 2, "字幕は下半分: {top}..{bottom}");
        }
    }
}
