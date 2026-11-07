//! 文字起こしの単語を `cutWords` で消すと、書き出しからその音が抜けて詰まり、1 回の取り消しで戻る。
//! 音は消す 2 語の間だけ鳴るトーンにして、書き出しの音量の最大で「消した音が入らない」を見る。
//! FFmpeg が要るので feature `ffmpeg` のときだけ走る。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mm_store::Store;
use mm_types::{Id, RationalTime};
use serde_json::{json, Value};

/// 素材の長さ（秒）。文字起こしのフィクスチャは 5 秒弱。
const MEDIA_SECONDS: &str = "6";
/// 話題にする単語の位置（「これは」から「です。」まで）。
const TOPIC_WORDS: (usize, usize) = (1, 17);
/// 消す 2 語の位置（「編」「集」）。
const CUT_WORDS: (usize, usize) = (3, 4);
/// トーンを消す範囲の内側に寄せる量（秒）。境界のフレームの丸めで鳴らないようにする。
const TONE_INSET: f64 = 0.05;
/// 書き出しの長さ（秒）の許容。1 フレームの丸めと AAC の先頭の無音。
const DURATION_TOLERANCE: f64 = 0.1;
/// トーンが鳴っているとみなす音量の最大（dBFS）。
const LOUD_DB: f64 = -30.0;
/// トーンが無いとみなす音量の最大（dBFS）。
const QUIET_DB: f64 = -60.0;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest")
}

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

fn secs(t: RationalTime) -> f64 {
    t.to_f64()
}

/// 映像（320x180）と、`[from, to]` の間だけ鳴る 1 kHz のトーンの素材。
fn make_media(out: &Path, from: f64, to: f64) {
    let tone = format!("sine=frequency=1000:sample_rate=48000:duration={MEDIA_SECONDS}");
    let gate = format!("volume=volume=0:enable='not(between(t,{from},{to}))'");
    run(Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=320x180:r=30:d={MEDIA_SECONDS}"))
        .args([
            "-f",
            "lavfi",
            "-i",
            &tone,
            "-af",
            &gate,
            "-t",
            MEDIA_SECONDS,
        ])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-c:a", "aac"])
        .arg(out));
}

/// フィクスチャの文字起こしの単語の時刻（並びの順）。
fn fixture_times() -> Vec<(RationalTime, RationalTime)> {
    let text = std::fs::read_to_string(fixtures_dir().join("transcript-ja.json")).unwrap();
    let sidecar: Value = serde_json::from_str(&text).unwrap();
    let words = sidecar["segments"].as_array().unwrap().iter();
    words
        .flat_map(|s| s["words"].as_array().unwrap().clone())
        .map(|w| {
            let t = |v: &Value| serde_json::from_value::<RationalTime>(v.clone()).unwrap();
            (t(&w["start"]), t(&w["end"]))
        })
        .collect()
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

/// 素材の ID と、DB の単語の ID（並びの順）。
fn asset_and_words(dir: &Path) -> (String, Vec<String>) {
    let store = Store::open_read_only(dir).unwrap();
    let asset = store.project().assets[0].id.clone();
    let words = store.transcript_words(&asset).unwrap();
    let ids = words.iter().map(|w| w.word.to_string()).collect();
    (asset.to_string(), ids)
}

/// 話題を 1 つ置いた sequence s1 と、それを長さいっぱいに置いたコンポジションとレンダーターゲット。
fn add_output(dir: &str, asset: &str, topic: (RationalTime, RationalTime)) {
    let (from, to) = topic;
    let length = to.checked_sub(from).unwrap();
    op(
        dir,
        json!({"op": "addTopic", "topic": {"id": "t1", "asset": asset,
        "in": from, "out": to, "title": "t1"}}),
    );
    op(
        dir,
        json!({"op": "addSequence", "sequence": {"id": "s1", "cuts": []}}),
    );
    op(
        dir,
        json!({"op": "insertTopics", "sequence": "s1", "topics": ["t1"], "cuts": ["k1"]}),
    );
    let source = json!({"kind": "sequence", "version": 1,
        "params": {"sequence": {"value": {"type": "text", "value": "s1"}}}});
    let layer = json!({"id": "l1", "source": source, "start": RationalTime::ZERO,
        "in": RationalTime::ZERO, "out": length,
        "opacity": {"value": {"type": "number", "value": 1.0}}});
    op(
        dir,
        json!({"op": "addComposition", "composition": {"id": "main", "width": 320,
        "height": 180, "fps": {"num": 30, "den": 1}, "duration": length, "layers": [layer]}}),
    );
    op(
        dir,
        json!({"op": "addRenderTarget", "target": {"id": "mp4", "composition": "main",
        "width": 320, "height": 180, "fps": {"num": 30, "den": 1}, "codec": "h264",
        "output_path": "out.mp4"}}),
    );
}

fn probe_duration(path: &Path) -> f64 {
    let out = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path));
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// 音の最大の音量（dBFS）。`volumedetect` の `max_volume`。
fn max_volume(path: &Path) -> f64 {
    let out = run(Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-af", "volumedetect", "-vn", "-f", "null", "-"]));
    let log = String::from_utf8_lossy(&out.stderr).into_owned();
    let line = log
        .lines()
        .find_map(|l| l.split("max_volume:").nth(1))
        .unwrap_or_else(|| panic!("max_volume が無い: {log}"));
    line.trim().trim_end_matches("dB").trim().parse().unwrap()
}

/// 書き出して、長さ（秒）と音量の最大（dBFS）を返す。
fn render(dir: &str, out: &Path) -> (f64, f64) {
    mm(&["render", dir, "-o", out.to_str().unwrap()]);
    (probe_duration(out), max_volume(out))
}

#[test]
fn 話題の中の_2_語を消すと書き出しがその分だけ短くなり_音が抜け_取り消し_1_回で戻る() {
    let times = fixture_times();
    let (cut_from, cut_to) = (times[CUT_WORDS.0].0, times[CUT_WORDS.1].1);
    let topic = (times[TOPIC_WORDS.0].0, times[TOPIC_WORDS.1].1);
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let media = tmp.path().join("talk.mp4");
    make_media(
        &media,
        secs(cut_from) + TONE_INSET,
        secs(cut_to) - TONE_INSET,
    );
    let d = dir.to_str().unwrap();
    mm(&["new", d]);
    place_transcript(&dir, &media);
    mm(&["ingest", d, media.to_str().unwrap(), "--encoder", "x264"]);
    let (asset, words) = asset_and_words(&dir);
    add_output(d, &asset, topic);

    let (full, loud) = render(d, &tmp.path().join("full.mp4"));
    let topic_len = secs(topic.1) - secs(topic.0);
    assert!(
        (full - topic_len).abs() < DURATION_TOLERANCE,
        "長さ {full}（{topic_len} のはず）"
    );
    assert!(loud > LOUD_DB, "消す前はトーンが鳴る: {loud} dB");

    let picked = [&words[CUT_WORDS.0], &words[CUT_WORDS.1]];
    op(
        d,
        json!({"op": "cutWords", "asset": asset, "sequence": "s1", "words": picked,
        "ranges": [{"in": cut_from, "out": cut_to}]}),
    );
    let (cut, quiet) = render(d, &tmp.path().join("cut.mp4"));
    let removed = secs(cut_to) - secs(cut_from);
    eprintln!("話題 {topic_len} 秒、消した 2 語 {removed} 秒、書き出し {full} 秒 → {cut} 秒、音量の最大 {loud} → {quiet} dB");
    assert!(
        (full - cut - removed).abs() < DURATION_TOLERANCE,
        "{full} 秒から {removed} 秒だけ短くなるはずが {cut} 秒"
    );
    assert!(quiet < QUIET_DB, "消した音が入っている: {quiet} dB");
    let store = Store::open_read_only(&dir).unwrap();
    let shown = store.transcript_words(&Id::new(asset.as_str())).unwrap();
    assert!(shown[CUT_WORDS.0].deleted && shown[CUT_WORDS.1].deleted);
    drop(store);

    mm(&["undo", d]);
    let (back, loud_again) = render(d, &tmp.path().join("undo.mp4"));
    assert!(
        (back - full).abs() < DURATION_TOLERANCE,
        "取り消して {back} 秒（{full} 秒のはず）"
    );
    assert!(
        loud_again > LOUD_DB,
        "取り消すとトーンが戻る: {loud_again} dB"
    );
}
