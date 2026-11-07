//! 段階 4 の終わりの条件「1 つのプロジェクトから本編とショートを書き出せ、プレビューと一致する」の通し。
//! 段階 3 の通しの素材（10 分を超える素材と文字起こし）から、本編（16:9、1080p）を話題で組み、その先頭の 2 話題を
//! 参照するショート（9:16、1080x1920、再フレーミングと字幕の置き場所つき）を作る。本編をリップルするとショートの
//! 従うカットだけが変わることを確かめ、2 つのレンダーターゲット（YouTube 用、Shorts 用）で書き出して次を確かめる。
//! - 書き出したフレームが `mm frame`（プレビューと同じレンダラーの経路）のフレームと SSIM で一致する。
//! - ショートのフレームが、本編の同じ素材の時刻のフレームを再フレーミングした画像と一致する。
//! - 字幕の時刻、チャプター、ラウドネス（-14 LUFS、-1 dBTP）、長さ、フレームの数。
//!
//! FFmpeg が要るので feature `ffmpeg` のときだけ実行する。環境変数は段階 3 の通し（`stage3_e2e`）と同じ。
//! 既定は縦横を 1/3 にした大きさ。`MM_E2E_FULL_HD=1` で本編 1080p、ショート 1080x1920。`MM_E2E_DIR` を渡すと、プロジェクトと書き出しを残す（アプリの `--play-test` で続きを試す）。

use super::stage3_e2e::{media, plan};

mod checks;
mod edit;
mod frames;

use std::path::{Path, PathBuf};
use std::time::Instant;

use mm_doc::{Project, Sequence};
use mm_eval::sequence_duration;
use mm_store::{Store, TranscriptWord};
use mm_types::{Id, RationalTime};
use serde_json::{json, Value};

use checks::{check_cues, ffprobe_json, frame_count, loudness, parse_srt, Timeline};
use edit::{
    apply_txn, build_main, build_short, mm, mm_try, project_of, ripple_main, short_cut_ids, Env,
    FPS, ORDER, RIPPLE_FOLLOWED, STUTTER_TOPIC,
};

/// 両方のレンダーターゲットのラウドネスの目標（YouTube と Shorts は -14 LUFS）。
const LUFS: f64 = -14.0;
/// 統合ラウドネスの許容（LU）。EBU R128 の許容。
const LUFS_TOLERANCE: f64 = 0.5;
/// トゥルーピークの上限（dBTP）。ffmpeg の測りと AAC の誤差の余裕を足す。
const PEAK_LIMIT: f64 = -0.9;
/// 書き出しの長さ（秒）の許容。1 フレームの丸めと AAC の先頭の無音。
const DURATION_TOLERANCE: f64 = 0.1;

/// 取り込みから文字起こしまで。素材の長さ（秒）を返す。
fn ingest_and_transcribe(env: &Env, report: &mut Value) -> f64 {
    let media_path = env.root.join("talk.mp4");
    let seconds = media::make_media(&media_path);
    assert!(seconds >= 600.0, "素材は 10 分以上: {seconds}");
    let dir = env.dir.as_str();
    mm(&["new", dir]);
    let ingest = |label: &str| {
        let started = Instant::now();
        mm(&[
            "ingest",
            dir,
            media_path.to_str().unwrap(),
            "--encoder",
            "x264",
        ]);
        eprintln!("{label}: {:.1} 秒", started.elapsed().as_secs_f64());
    };
    ingest("取り込み");
    match std::env::var("MM_E2E_PYTHON") {
        Ok(python) => {
            media::transcribe(&python, Path::new(dir), &media_path);
            report["transcriber"] = json!("faster-whisper");
        }
        Err(_) => {
            media::write_synthetic_transcript(Path::new(dir), &media_path, seconds);
            report["transcriber"] = json!("synthetic");
        }
    }
    ingest("文字起こしの読み込み");
    seconds
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

fn cut_range(p: &Project, id: &str) -> (RationalTime, RationalTime) {
    let cut = p.cut(&Id::new(id)).unwrap().1;
    (cut.in_point, cut.out_point)
}

fn comp_duration(p: &Project, id: &str) -> RationalTime {
    p.composition(&Id::new(id)).unwrap().duration
}

fn seq_duration(p: &Project, id: &str) -> RationalTime {
    sequence_duration(p.sequence(&Id::new(id)).unwrap()).unwrap()
}

/// 言い淀みを抜く（ショートが参照しない話題の中）。
fn remove_stutter(env: &Env, asset: &str, words: &[TranscriptWord], topics: &[plan::Topic]) {
    let (at, _) = plan::pick_stutter(words, &topics[STUTTER_TOPIC]);
    apply_txn(
        env,
        "stutter",
        plan::delete_stutter(asset, "s1", &words[at]),
    );
}

/// 本編のリップルにショートが従い、上書きしたカットは変わらず、コンポジションの長さが追従すること。
fn ripple_and_check(env: &Env) {
    let ids = short_cut_ids();
    let before = project_of(&env.dir);
    let (main_before, r_before) = (cut_range(&before, &ids[0].0), cut_range(&before, &ids[0].1));
    let overridden_before = cut_range(&before, &ids[1].1);
    ripple_main(env);
    let after = project_of(&env.dir);
    let shrunk = RationalTime::seconds(RIPPLE_FOLLOWED);
    assert_eq!(
        cut_range(&after, &ids[0].0).1,
        main_before.1.checked_sub(shrunk).unwrap()
    );
    assert_eq!(
        cut_range(&after, &ids[0].1),
        (r_before.0, r_before.1.checked_sub(shrunk).unwrap()),
        "従うカットは本編のリップルに従う"
    );
    assert_eq!(
        cut_range(&after, &ids[1].1),
        overridden_before,
        "上書きしたカットは本編のリップルで変わらない"
    );
    assert_ne!(cut_range(&after, &ids[1].0), cut_range(&before, &ids[1].0));
    for (comp, seq) in [("main", "s1"), ("vert", "short")] {
        assert_eq!(
            comp_duration(&after, comp),
            seq_duration(&after, seq),
            "{comp} の長さが追従する"
        );
    }
}

/// 書き出しの長さとフレームの数。
fn check_lengths(path: &Path, expected: f64) {
    let fps = FPS as f64;
    let n = frame_count(path);
    assert!(
        (n as f64 - expected * fps).abs() <= 1.0,
        "{n} フレーム（{} のはず）",
        expected * fps
    );
    for (what, kind) in [("全体", None), ("映像", Some("v")), ("音", Some("a"))] {
        let got = match kind {
            None => media::probe_duration(path),
            Some(k) => media::probe_stream_duration(path, k).expect("ストリーム"),
        };
        assert!(
            (got - expected).abs() < DURATION_TOLERANCE,
            "{what}の長さ {got} 秒（{expected} 秒のはず）"
        );
    }
}

fn check_loudness(path: &Path, report: &mut Value, key: &str) {
    let (lufs, peak) = loudness(path);
    eprintln!("{key}: 統合ラウドネス {lufs:.2} LUFS（目標 {LUFS}）、ピーク {peak:.2} dBFS");
    report[key] = json!({"lufs": lufs, "peak_dbfs": peak});
    assert!((lufs - LUFS).abs() <= LUFS_TOLERANCE, "{key}: {lufs} LUFS");
    assert!(peak <= PEAK_LIMIT, "{key}: {peak} dBFS");
}

/// チャプターが本編のマーカーの時刻に合い、説明欄の文字列が出ること。
fn check_chapters(p: &Project, yt: &Path) {
    let marks = edit::chapter_marks(p);
    let chapters = ffprobe_json(yt, &["-show_chapters"]);
    let chapters = chapters["chapters"].as_array().unwrap();
    assert_eq!(chapters.len(), marks.len(), "{chapters:?}");
    let mut text = String::new();
    for (c, (_, at, name)) in chapters.iter().zip(&marks) {
        let got: f64 = c["start_time"].as_str().unwrap().parse().unwrap();
        eprintln!(
            "チャプター {name}: {got:.3} 秒（マーカー {:.3} 秒）",
            at.to_f64()
        );
        assert!(
            (got - at.to_f64()).abs() < checks::TIME_TOLERANCE,
            "チャプター {name} の始まり {got}"
        );
        assert_eq!(c["tags"]["title"], name.as_str());
        let s = at.to_f64().floor() as i64;
        text += &format!("{:02}:{:02} {name}\n", s / 60, s % 60);
    }
    let file = std::fs::read_to_string(yt.with_extension("chapters.txt")).unwrap();
    assert_eq!(file, text);
}

/// 書き出しの隣の SRT を読んで確かめる。字幕の数と、語の番号まで確かめた数を返す。
fn check_sidecar(out: &Path, seq: &Sequence, words: &[TranscriptWord]) -> (usize, usize) {
    let cues = parse_srt(&std::fs::read_to_string(out.with_extension("srt")).unwrap());
    let vtt = std::fs::read_to_string(out.with_extension("vtt")).unwrap();
    assert!(vtt.starts_with("WEBVTT\n"));
    assert_eq!(vtt.matches(" --> ").count(), cues.len());
    let starts: Vec<f64> = words.iter().map(|w| w.start.to_f64()).collect();
    check_cues(&cues, &Timeline::of(seq), &starts)
}

#[test]
#[ignore = "通しの試験。main への push と手動実行で走らせる"]
fn 本編とショートを書き出し_プレビューと一致する() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Env::new(work_root(&tmp));
    let mut report = json!({});

    let seconds = ingest_and_transcribe(&env, &mut report);
    let store = Store::open_read_only(Path::new(&env.dir)).unwrap();
    let asset = store.project().assets[0].id.to_string();
    let words = store.transcript_words(&Id::new(asset.as_str())).unwrap();
    drop(store);

    let topics = plan::pick_topics(&words, seconds);
    apply_txn(
        &env,
        "topics",
        topics.iter().map(|t| plan::add_topic(&asset, t)).collect(),
    );
    let total = build_main(&env, &topics);
    assert_eq!(
        comp_duration(&project_of(&env.dir), "main"),
        total,
        "本編の長さが話題の和まで伸びる"
    );
    remove_stutter(&env, &asset, &words, &topics);
    build_short(&env, &topics);
    ripple_and_check(&env);
    edit::add_targets(&env, LUFS);
    let p = project_of(&env.dir);
    report["main_secs"] = json!(comp_duration(&p, "main").to_f64());
    report["short_secs"] = json!(comp_duration(&p, "vert").to_f64());
    report["order"] = json!(ORDER);

    let yt = env.root.join("yt.mp4");
    let shorts = env.root.join("shorts.mp4");
    let codec = render_yt(&env, &yt, &mut report);
    report["yt_codec"] = json!(codec);
    let started = Instant::now();
    mm(&[
        "render",
        &env.dir,
        "--target",
        "shorts",
        "-o",
        shorts.to_str().unwrap(),
    ]);
    let secs = started.elapsed().as_secs_f64();
    report["shorts_render"] =
        json!({"secs": secs, "fps": comp_duration(&p, "vert").to_f64() * FPS as f64 / secs});

    check_lengths(&yt, comp_duration(&p, "main").to_f64());
    check_lengths(&shorts, comp_duration(&p, "vert").to_f64());
    check_loudness(&yt, &mut report, "yt_loudness");
    check_loudness(&shorts, &mut report, "shorts_loudness");
    check_chapters(&p, &yt);
    let main_seq = p.sequence(&Id::new("s1")).unwrap();
    let short_seq = p.sequence(&Id::new("short")).unwrap();
    let main_cues = check_sidecar(&yt, main_seq, &words);
    let short_cues = check_sidecar(&shorts, short_seq, &words);
    report["cues"] = json!({"main": main_cues.0, "main_numbered": main_cues.1,
        "short": short_cues.0, "short_numbered": short_cues.1});

    frames::check_main(&env, &p, &yt, &mut report);
    frames::check_short(&env, &p, &shorts, &mut report);

    if let Ok(path) = std::env::var("MM_E2E_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    eprintln!("{report:#}");
}

/// YouTube 用を書き出す。NVENC があれば NVENC、無ければ libx264。使ったコーデックと書き出しの fps を `report` に足す。
fn render_yt(env: &Env, out: &Path, report: &mut Value) -> &'static str {
    let o = out.to_str().unwrap();
    let mut codec = "h264_nvenc";
    let started = Instant::now();
    let tried = mm_try(&[
        "render", &env.dir, "--target", "yt", "-o", o, "--codec", codec,
    ]);
    if !tried.status.success() {
        eprintln!(
            "NVENC を使えない: {}",
            String::from_utf8_lossy(&tried.stderr)
        );
        codec = "h264";
        mm(&[
            "render", &env.dir, "--target", "yt", "-o", o, "--codec", codec,
        ]);
    }
    let secs = started.elapsed().as_secs_f64();
    let len = comp_duration(&project_of(&env.dir), "main").to_f64();
    report["yt_render"] = json!({"secs": secs, "fps": len * FPS as f64 / secs});
    codec
}
