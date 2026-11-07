//! 「長い素材から話題を切り出して並べ、そのまま再生できる」の通し。
//! 素材を `mm ingest` で取り込んで文字起こしを DB に入れ、`mm search` で引き、話題を足し、
//! 話題から sequence を作って `mm render` で書き出す。FFmpeg が要るので feature `ffmpeg` のときだけ走る。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

/// 素材の長さ（秒）。文字起こしのフィクスチャは 5 秒弱。
const MEDIA_SECONDS: &str = "6";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest")
}

fn mm(args: &[&str]) -> Output {
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
    out
}

/// 映像（320x180）と音の素材。`fixtures/ingest/generate.sh` の日本語の音声があればそれを、無ければトーンを使う。
fn make_media(out: &Path) {
    let speech = fixtures_dir().join("out/speech-ja.wav");
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=320x180:r=30:d={MEDIA_SECONDS}"));
    if speech.is_file() {
        cmd.arg("-i").arg(&speech).args(["-af", "apad"]);
    } else {
        cmd.args(["-f", "lavfi", "-i"]).arg(format!(
            "sine=frequency=440:sample_rate=48000:duration={MEDIA_SECONDS}"
        ));
    }
    let status = cmd
        .args([
            "-t",
            MEDIA_SECONDS,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
        ])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(out)
        .status()
        .expect("ffmpeg を実行できる");
    assert!(status.success());
}

/// フィクスチャの文字起こしを、素材のコンテンツハッシュに付け替えて `cache/ingest/` に置く（ツールが書いた形）。
fn place_transcript(dir: &Path, media: &Path) {
    let hash = mm_ingest::hash_file(media).unwrap();
    let text = std::fs::read_to_string(fixtures_dir().join("transcript-ja.json")).unwrap();
    let mut sidecar: Value = serde_json::from_str(&text).unwrap();
    sidecar["content_hash"] = json!(hash);
    let path = mm_ingest::CacheLayout::new(dir.join("cache")).sidecar(&hash, "transcript");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, sidecar.to_string()).unwrap();
}

/// `mm search --json` の最初の 1 件。
fn first_hit(dir: &str, query: &str) -> Value {
    let out = mm(&["search", dir, query, "--json"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text
        .lines()
        .next()
        .unwrap_or_else(|| panic!("{query} が引けない"));
    serde_json::from_str(line).unwrap()
}

fn seconds(t: &Value) -> f64 {
    t["num"].as_f64().unwrap() / t["den"].as_f64().unwrap()
}

fn op(dir: &str, name: &str, args: Value) {
    mm(&["op", dir, name, "--json", &args.to_string()]);
}

fn add_topic(dir: &str, id: &str, from: &Value, to: &Value) {
    let topic = json!({
        "id": id, "asset": from["asset"], "in": from["start"], "out": to["end"], "title": id,
    });
    op(dir, "addTopic", json!({ "topic": topic }));
}

/// 話題を並べた sequence を入れ子にするコンポジションとレンダーターゲット。
fn add_output(dir: &str, length: &Value) {
    op(
        dir,
        "addSequence",
        json!({"sequence": {"id": "s1", "cuts": []}}),
    );
    op(
        dir,
        "insertTopics",
        json!({"sequence": "s1", "topics": ["t2", "t1"], "cuts": ["k2", "k1"]}),
    );
    let source = json!({"kind": "sequence", "version": 1,
        "params": {"sequence": {"value": {"type": "text", "value": "s1"}}}});
    let layer = json!({"id": "l1", "source": source, "start": {"num": 0, "den": 1},
        "in": {"num": 0, "den": 1}, "out": length, "opacity": {"value": {"type": "number", "value": 1.0}}});
    let comp = json!({"id": "main", "width": 320, "height": 180, "fps": {"num": 30, "den": 1},
        "duration": length, "layers": [layer]});
    op(dir, "addComposition", json!({ "composition": comp }));
    let target = json!({"id": "mp4", "composition": "main", "width": 320, "height": 180,
        "fps": {"num": 30, "den": 1}, "codec": "h264", "output_path": "out.mp4"});
    op(dir, "addRenderTarget", json!({ "target": target }));
}

fn probe_duration(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe を実行できる");
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn 取り込んだ文字起こしを引いて話題を並べ_書き出せる() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let media = tmp.path().join("talk.mp4");
    make_media(&media);
    let d = dir.to_str().unwrap();
    mm(&["new", d]);
    place_transcript(&dir, &media);
    let ingest = mm(&["ingest", d, media.to_str().unwrap(), "--encoder", "x264"]);
    let ingest = String::from_utf8_lossy(&ingest.stdout).into_owned();
    assert!(
        ingest.contains("文字起こし 18 語を DB に入れた"),
        "{ingest}"
    );

    // 「編集」は「編」と「集」の 2 つの単語にまたがる。
    let edit = first_hit(d, "編集");
    assert_eq!(edit["start"], json!({"num": 111, "den": 50}));
    assert_eq!(edit["end"], json!({"num": 137, "den": 50}));
    add_topic(d, "t1", &first_hit(d, "動画編集"), &first_hit(d, "ソフト"));
    add_topic(
        d,
        "t2",
        &first_hit(d, "文字起こし"),
        &first_hit(d, "試験です"),
    );
    let t1 = seconds(&first_hit(d, "ソフト")["end"]) - seconds(&first_hit(d, "動画編集")["start"]);
    let t2 =
        seconds(&first_hit(d, "試験です")["end"]) - seconds(&first_hit(d, "文字起こし")["start"]);
    // 2 つの話題の長さの和を、コンポジションの長さに（フレームに丸めた 1/30 秒の単位で）。
    let frames = ((t1 + t2) * 30.0).floor() as i64;
    add_output(d, &json!({"num": frames, "den": 30}));

    let out = tmp.path().join("topics.mp4");
    mm(&["render", d, "-o", out.to_str().unwrap()]);
    let duration = probe_duration(&out);
    let want = frames as f64 / 30.0;
    assert!(
        (duration - want).abs() < 0.1,
        "長さ {duration} 秒（{want} 秒のはず）"
    );
}
