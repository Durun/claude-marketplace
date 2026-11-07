//! 通しの試験の素材と文字起こし。素材は 10 分を超える長さで、音は日本語の話（無ければトーン）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::{json, Value};

/// 日本語の話が無いときの素材の長さ（秒）。
const FALLBACK_SECONDS: u32 = 620;
/// 合成の文字起こしの単語の間隔（5 分の 3 秒）。
const WORD_STEP: (i64, i64) = (3, 5);
/// 合成の文字起こしで、1 つの区間に入れる単語の数。
const WORDS_PER_SEGMENT: usize = 12;
/// 合成の文字起こしで、何語ごとに言い淀みの語を入れるか。
const FILLER_EVERY: usize = 9;

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest")
}

/// 素材の長さ（秒）。
pub fn probe_duration(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration"])
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("ffprobe を実行できる");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{} の長さを読めない", path.display()))
}

/// ストリームごとの長さ（秒）。`kind` は `v` か `a`。ストリームが無ければ `None`。
pub fn probe_stream_duration(path: &Path, kind: &str) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", kind])
        .args(["-show_entries", "stream=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("ffprobe を実行できる");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|l| l.trim().parse().ok())
}

/// 映像（640x360、30fps）と音の素材を作り、長さ（秒）を返す。
/// `fixtures/ingest/generate.sh` が作る `speech-long-ja.wav` があればそれを、無ければトーンを音にする。
pub fn make_media(out: &Path) -> f64 {
    let speech = fixtures_dir().join("out/speech-long-ja.wav");
    let seconds = if speech.is_file() {
        probe_duration(&speech).ceil() as u32
    } else {
        FALLBACK_SECONDS
    };
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=640x360:r=30:d={seconds}"));
    if speech.is_file() {
        cmd.arg("-i").arg(&speech).args(["-af", "apad"]);
    } else {
        cmd.args(["-f", "lavfi", "-i"]).arg(format!(
            "sine=frequency=440:sample_rate=48000:duration={seconds}"
        ));
    }
    let status = cmd
        .args(["-t", &seconds.to_string(), "-c:v", "libx264"])
        .args(["-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(out)
        .status()
        .expect("ffmpeg を実行できる");
    assert!(status.success());
    probe_duration(out)
}

/// 文字起こしの付属ファイルを `cache/ingest/` に置く（ツールが書く形）。
pub fn sidecar_path(project: &Path, hash: &str) -> PathBuf {
    mm_ingest::CacheLayout::new(project.join("cache")).sidecar(hash, "transcript")
}

/// 音声認識の代わりに、一定の間隔の単語と、言い淀みの語を並べた文字起こしを置く。
pub fn write_synthetic_transcript(project: &Path, media: &Path, seconds: f64) {
    let hash = mm_ingest::hash_file(media).expect("素材のハッシュを取れる");
    let time = |n: i64| json!({"num": n * WORD_STEP.0, "den": WORD_STEP.1});
    let count = (seconds * WORD_STEP.1 as f64 / WORD_STEP.0 as f64) as i64 - 1;
    let words: Vec<Value> = (0..count)
        .map(|i| {
            let text = if i as usize % FILLER_EVERY == FILLER_EVERY - 1 {
                "えーと".to_owned()
            } else {
                format!("語{i}")
            };
            json!({"start": time(i), "end": time(i + 1), "text": text})
        })
        .collect();
    let segments: Vec<Value> = words
        .chunks(WORDS_PER_SEGMENT)
        .map(|chunk| {
            let text: String = chunk.iter().filter_map(|w| w["text"].as_str()).collect();
            json!({"start": chunk[0]["start"], "end": chunk[chunk.len() - 1]["end"],
                   "text": text, "words": chunk})
        })
        .collect();
    let sidecar = json!({"schema_version": 1, "content_hash": hash, "producer": "synthetic",
        "kind": "transcript", "language": "ja", "segments": segments});
    let path = sidecar_path(project, &hash);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, sidecar.to_string()).unwrap();
}

/// `tools/transcribe`（faster-whisper、GPU）で文字起こしし、かかった時間（秒）を返す。
pub fn transcribe(python: &str, project: &Path, media: &Path) -> f64 {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/transcribe/transcribe.py");
    let started = Instant::now();
    let out = Command::new(python)
        .arg("-I")
        .arg(script)
        .arg(media)
        .arg("--cache")
        .arg(project.join("cache"))
        .args(["--language", "ja"])
        .output()
        .expect("文字起こしのツールを実行できる");
    assert!(
        out.status.success(),
        "文字起こしが失敗した: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    started.elapsed().as_secs_f64()
}
