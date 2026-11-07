//! 段階 3 の終わりの条件「長い素材から話題を切り出して並べ、そのまま再生できる」の通し（CLI の部分）。
//! 10 分を超える素材を `mm ingest` で取り込み、文字起こしし、話題を 6 つ作り、並べ替えて sequence にし、
//! 言い淀みを抜いて `mm render` で書き出し、長さが話題の和に合うことを確かめる。FFmpeg が要るので feature `ffmpeg` のときだけ走る。
//!
//! 環境変数
//! - `MM_E2E_PYTHON`: `tools/transcribe` の依存を入れた Python（GPU）。無ければ合成の文字起こしを使う。
//! - `MM_E2E_DIR`: プロジェクトと書き出しをこのフォルダーに残す（アプリの `--play-test` で続きを試す）。
//! - `MM_E2E_REPORT`: 時間などの数字を JSON で書く先。

pub(super) mod media;
pub(super) mod plan;

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Instant;

use mm_store::{Store, TranscriptWord};
use mm_types::{Id, RationalTime};
use serde_json::{json, Value};

use plan::Topic;

/// 書き出しの長さ（秒）の許容。1 フレーム（1/30 秒）の丸めと、AAC の先頭の無音。
const DURATION_TOLERANCE: f64 = 0.1;
/// 並べる順（話題の番号）。素材の順とは別にする。
const ORDER: [usize; plan::TOPICS] = [3, 1, 4, 0, 5, 2];
/// 言い淀みを抜く話題の番号。
const STUTTER_TOPIC: usize = 0;
/// 最初にコンポジションを作る話題の番号（`ORDER` の先頭）。
const FIRST_TOPIC: usize = ORDER[0];
/// 書き出しのフレームレート。
const FPS: f64 = 30.0;

fn mm(args: &[&str]) -> (Output, f64) {
    let started = Instant::now();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mm"))
        .args(args)
        .output()
        .expect("mm を実行できる");
    let secs = started.elapsed().as_secs_f64();
    assert!(
        out.status.success(),
        "mm {args:?} が失敗した: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, secs)
}

fn apply_txn(dir: &str, root: &Path, name: &str, ops: Vec<Value>) {
    let file = root.join(format!("{name}.json"));
    std::fs::write(&file, plan::txn(name, ops)).unwrap();
    mm(&["op", dir, "--txn", file.to_str().unwrap()]);
}

fn project_of(dir: &str) -> mm_doc::Project {
    Store::open_read_only(Path::new(dir))
        .expect("プロジェクトを開ける")
        .project()
        .clone()
}

fn comp_length(dir: &str) -> (RationalTime, RationalTime) {
    let p = project_of(dir);
    let comp = p.composition(&Id::new("main")).unwrap();
    let layer = comp.layer(&Id::new("l1")).unwrap();
    (comp.duration, layer.out_point)
}

fn sum(times: impl Iterator<Item = RationalTime>) -> RationalTime {
    times.fold(RationalTime::ZERO, |a, b| a.checked_add(b).unwrap())
}

fn close(a: f64, b: f64, what: &str) {
    assert!(
        (a - b).abs() < DURATION_TOLERANCE,
        "{what}: {a} 秒（{b} 秒のはず）"
    );
}

/// 取り込みから文字起こしまで。素材の長さ（秒）と、各段階の時間を返す。
fn ingest_and_transcribe(dir: &str, root: &Path, report: &mut Value) -> f64 {
    let media_path = root.join("talk.mp4");
    let seconds = media::make_media(&media_path);
    assert!(seconds >= 600.0, "素材は 10 分以上: {seconds}");
    report["media_secs"] = json!(seconds);
    mm(&["new", dir]);
    let (_, ingest) = mm(&[
        "ingest",
        dir,
        media_path.to_str().unwrap(),
        "--encoder",
        "x264",
    ]);
    report["ingest_secs"] = json!(ingest);
    match std::env::var("MM_E2E_PYTHON") {
        Ok(python) => {
            report["transcribe_secs"] =
                json!(media::transcribe(&python, Path::new(dir), &media_path));
            report["transcriber"] = json!("faster-whisper");
        }
        Err(_) => {
            media::write_synthetic_transcript(Path::new(dir), &media_path, seconds);
            report["transcriber"] = json!("synthetic");
        }
    }
    let (out, load) = mm(&[
        "ingest",
        dir,
        media_path.to_str().unwrap(),
        "--encoder",
        "x264",
    ]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        text.contains("文字起こし") && text.contains("語を DB に入れた"),
        "{text}"
    );
    report["transcript_load_secs"] = json!(load);
    seconds
}

/// プロジェクトのフォルダーと、作業のフォルダー。
struct Env {
    root: PathBuf,
    dir: String,
}

impl Env {
    fn new(root: PathBuf) -> Self {
        let dir = root.join("p").to_str().unwrap().to_owned();
        Self { root, dir }
    }
}

/// 話題を作る。素材の等分した位置から `plan::TOPICS` 個。
fn make_topics(env: &Env, asset: &str, words: &[TranscriptWord], seconds: f64) -> Vec<Topic> {
    let topics = plan::pick_topics(words, seconds);
    let adds = topics.iter().map(|t| plan::add_topic(asset, t)).collect();
    apply_txn(&env.dir, &env.root, "topics", adds);
    assert_eq!(project_of(&env.dir).bins.topics.len(), plan::TOPICS);
    topics
}

/// 話題を並べ替えて sequence にする。最初の話題でコンポジションを作ってから残りを足し、
/// コンポジションとレイヤーの長さが話題の和まで伸びること（`insertTopics` と同じトランザクションで追従）を確かめる。
fn arrange(env: &Env, topics: &[Topic]) -> RationalTime {
    let by_order: Vec<&Topic> = ORDER.iter().map(|i| &topics[*i]).collect();
    let seq = json!({"op": "addSequence", "sequence": {"id": "s1", "cuts": []}});
    let build = vec![
        seq,
        plan::insert_topics("s1", &by_order[..1]),
        plan::add_composition("s1", topics[FIRST_TOPIC].len()),
        plan::add_render_target(),
    ];
    apply_txn(&env.dir, &env.root, "build", build);
    let rest = vec![plan::insert_topics("s1", &by_order[1..])];
    apply_txn(&env.dir, &env.root, "arrange", rest);
    let total = sum(topics.iter().map(Topic::len));
    assert_eq!(comp_length(&env.dir), (total, total));
    total
}

/// 言い淀みを抜く。1 回の取り消しで、カットもコンポジションの長さも戻る。抜いた後の長さを返す。
fn remove_stutter(
    env: &Env,
    asset: &str,
    words: &[TranscriptWord],
    topics: &[Topic],
    total: RationalTime,
    report: &mut Value,
) -> RationalTime {
    let topic = &topics[STUTTER_TOPIC];
    let (at, is_filler) = plan::pick_stutter(words, topic);
    let word = &words[at];
    let cut_off = word.end.checked_sub(word.start).unwrap();
    report["stutter"] =
        json!({"text": word.shown(), "filler": is_filler, "secs": cut_off.to_f64()});
    apply_txn(
        &env.dir,
        &env.root,
        "stutter",
        plan::delete_stutter(asset, "s1", word),
    );
    let want = total.checked_sub(cut_off).unwrap();
    assert_eq!(comp_length(&env.dir), (want, want));
    mm(&["undo", &env.dir]);
    assert_eq!(comp_length(&env.dir), (total, total));
    mm(&["redo", &env.dir]);
    assert_eq!(comp_length(&env.dir), (want, want));
    want
}

/// 書き出して、長さが話題の和（言い淀みを抜いた後）に合うことを確かめる。
fn render_and_check(env: &Env, want: RationalTime, report: &mut Value) {
    let out = env.root.join("topics.mp4");
    let (_, render) = mm(&["render", &env.dir, "-o", out.to_str().unwrap()]);
    let expected = want.to_f64();
    report["render_secs"] = json!(render);
    report["render_fps"] = json!(expected * FPS / render);
    report["expected_secs"] = json!(expected);
    close(media::probe_duration(&out), expected, "書き出しの長さ");
    close(
        media::probe_stream_duration(&out, "v").expect("映像"),
        expected,
        "映像の長さ",
    );
    close(
        media::probe_stream_duration(&out, "a").expect("音"),
        expected,
        "音の長さ",
    );
}

fn work_root(keep: &tempfile::TempDir) -> PathBuf {
    match std::env::var("MM_E2E_DIR") {
        Ok(d) => {
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            PathBuf::from(d)
        }
        Err(_) => keep.path().to_path_buf(),
    }
}

#[test]
#[ignore = "通しの試験。main への push と手動実行で走らせる"]
fn 長い素材から話題を切り出して並べ_言い淀みを抜いて書き出せる() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Env::new(work_root(&tmp));
    let mut report = json!({});

    let seconds = ingest_and_transcribe(&env.dir, &env.root, &mut report);
    let store = Store::open_read_only(Path::new(&env.dir)).unwrap();
    let asset = store.project().assets[0].id.to_string();
    let words = store.transcript_words(&Id::new(asset.as_str())).unwrap();
    drop(store);
    report["words"] = json!(words.len());

    let topics = make_topics(&env, &asset, &words, seconds);
    let total = arrange(&env, &topics);
    report["topics_secs"] = json!(total.to_f64());
    let want = remove_stutter(&env, &asset, &words, &topics, total, &mut report);
    render_and_check(&env, want, &mut report);

    if let Ok(path) = std::env::var("MM_E2E_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    eprintln!("{report:#}");
}
