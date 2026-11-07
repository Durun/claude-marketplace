//! ep03（DTM_Vlog の作曲Vlog #3）を `mm geneva` で取り込み、`mm render` で書き出して前の書き出しと比べる
//! （`docs/design/11-testing.md` の「実データ」）。
//!
//! 素材はリポジトリに入れない。`MM_EP03_DIR` に回のフォルダー（`timeline.json` と `render/ep03_final.mp4` がある所）を
//! 渡したときだけ実行し、無ければ何もせず通る。既定は全体に散らした区間だけを書き出し、`MM_EP03_FULL=1` で全体を書き出す。

mod measure;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};

use measure::{Picture, Sound};

/// 回のフォルダーを渡す環境変数。
const ENV_DIR: &str = "MM_EP03_DIR";
/// 全体を書き出す環境変数。
const ENV_FULL: &str = "MM_EP03_FULL";
/// 区間の書き出しで比べる位置（全体の長さに対する割合）と、1 つの区間の秒数。
const WINDOW_AT: [f64; 6] = [0.02, 0.2, 0.4, 0.6, 0.8, 0.97];
const WINDOW_SECONDS: f64 = 4.0;
/// 区間の始まりをそろえる刻み（1 秒あたりの数）。区間を変えると前の回の数字と比べられないので、0.1 秒の格子に保つ。
const WINDOW_GRID: f64 = 10.0;

/// 区間ごとの平均の SSIM の下限。
/// 前の書き出し（geneva、H.264）と movie-maker の書き出し（H.264）は、どちらも非可逆の符号化を通るので一致しない。
/// 前の書き出しを movie-maker の既定（libx264、CRF 18、medium）で符号化し直しただけで 0.995 になり、
/// 再現の区間は 0.982〜0.993 だった。残りの差は描き方（縮小のフィルター）に充てる。
const SSIM_MEAN_MIN: f64 = 0.97;
/// 1 フレームの SSIM の下限。60 fps の OBS の収録（mkv の ms 単位の丸め）は、movie-maker が CFR の格子で、
/// geneva が PTS でフレームを選ぶので、3 フレームに 1 回 1/60 秒違うフレームになり、動きの大きい所で 0.88 まで下がる。
const SSIM_FRAME_MIN: f64 = 0.85;
/// 全体の書き出しの 1 フレームの SSIM の下限。全体には区間に入らない 60 fps の収録のスクロール（373.3 秒）があり、
/// 同じ理由で 0.818 まで下がる。
const SSIM_FRAME_MIN_FULL: f64 = 0.80;
/// 区間ごとの平均の PSNR の下限（dB）。再現の区間は 36.7〜41.4 dB だった。
const PSNR_MEAN_MIN: f64 = 33.0;
/// 音の差の下限（dB）。AAC の符号化を 2 回とリミッターを通るので一致しない。再現の区間は 18〜28 dB だった。
const SNR_MIN_DB: f64 = 15.0;
/// 50 ms ごとの大きさの差の平均の上限（dB）。再現の区間は 0.16 dB 以下。フェードが抜けると 2 dB を超える。
const ENVELOPE_MAX_DB: f64 = 0.5;
/// 音の時刻のずれの上限（ミリ秒）。AAC の前置きの扱いを誤ると約 21 ms、1 フレームずれると 33 ms になる。
const LAG_MAX_MS: f64 = 2.0;
/// 全体の書き出しのラウドネスの差の上限（dB）。区間の書き出しはラウドネスを区間で合わせるので見ない。
const GAIN_MAX_DB: f64 = 1.0;
/// 書き出しの音のサンプルのピークの上限（dBFS）。正規化は -1 dBTP に収めるので、0 dBFS を超えるのは
/// エンコーダーが作ったクリック（AAC の雑音の置き換えで、片方のチャンネルに数サンプル +4.6 dBFS が出た）。
const PEAK_MAX_DBFS: f64 = 0.0;

/// 変換できない要素として認める一覧（種類、数）。増えたら変換か movie-maker の機能を見直す。
const KNOWN_GAPS: [(&str, u64); 0] = [];

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ep03/expected.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("expected.json がある"))
        .expect("JSON")
}

fn sha256(path: &Path) -> String {
    let mut file = std::fs::File::open(path).expect("開ける");
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).expect("読める");
    format!("{:x}", hasher.finalize())
}

/// 入力と前の書き出しが、フィクスチャに書いたものと同じか確かめてパスを返す。
fn inputs(dir: &Path, expected: &Value) -> (PathBuf, PathBuf) {
    let check = |key: &str| {
        let path = dir.join(expected[key]["path"].as_str().expect("path"));
        let want = expected[key]["sha256"].as_str().expect("sha256");
        assert_eq!(sha256(&path), want, "{} が変わった", path.display());
        path
    };
    (check("timeline"), check("reference"))
}

fn mm(args: &[&OsStr]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mm"))
        .args(args)
        .output()
        .expect("mm を実行できる");
    assert!(
        out.status.success(),
        "mm {:?} が成功する: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 取り込んでプロジェクトの JSON を書き、要約を返す。
fn import(timeline: &Path, project: &Path) -> Value {
    let summary = mm(&[
        OsStr::new("geneva"),
        timeline.as_os_str(),
        OsStr::new("-o"),
        project.as_os_str(),
    ]);
    serde_json::from_str(summary.trim()).expect("要約の JSON")
}

fn render(project: &Path, out: &Path, range: Option<(f64, f64)>) {
    let range = range.map(|(a, b)| format!("{a}:{b}"));
    let mut args: Vec<&OsStr> = vec![
        OsStr::new("render"),
        project.as_os_str(),
        OsStr::new("-o"),
        out.as_os_str(),
    ];
    if let Some(r) = &range {
        args.push(OsStr::new("--range"));
        args.push(OsStr::new(r));
    }
    mm(&args);
}

/// 区間（秒）。`full` なら全体 1 つ、そうでなければ短い区間を散らす（始まりは 0.1 秒の倍数）。
fn windows(duration: f64, full: bool) -> Vec<(f64, f64)> {
    if full {
        return vec![(0.0, duration)];
    }
    WINDOW_AT
        .iter()
        .map(|at| {
            let start = ((duration - WINDOW_SECONDS) * at * WINDOW_GRID).floor() / WINDOW_GRID;
            (start, start + WINDOW_SECONDS)
        })
        .collect()
}

fn check_gaps(summary: &Value) {
    let gaps: Vec<(String, u64)> = summary["gaps"]
        .as_array()
        .expect("gaps")
        .iter()
        .map(|g| {
            (
                g["what"].as_str().unwrap_or("").to_owned(),
                g["count"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    let known: Vec<(String, u64)> = KNOWN_GAPS
        .iter()
        .map(|(w, n)| ((*w).to_owned(), *n))
        .collect();
    assert_eq!(gaps, known, "変換できない要素の一覧が変わった");
}

fn report(range: (f64, f64), p: &Picture, s: &Sound) {
    eprintln!(
        "[{:7.2}, {:7.2}) 映像 {} フレーム SSIM 平均 {:.4} 最小 {:.4} PSNR {:.2} dB / 音 倍率 {:+.2} dB ずれ {:+.2} ms SNR {:.1} dB 包絡 {:.2} dB ピーク {:+.2} dBFS",
        range.0, range.1, p.frames, p.ssim_mean, p.ssim_min, p.psnr_mean, s.gain_db, s.lag_ms, s.snr_db, s.envelope_db, s.peak_dbfs
    );
}

/// 閾値を割った項目の一覧。全部の区間を見てから失敗させるため、ここでは集めるだけ。
fn misses(range: (f64, f64), p: &Picture, s: &Sound, full: bool) -> Vec<String> {
    let frame_min = if full {
        SSIM_FRAME_MIN_FULL
    } else {
        SSIM_FRAME_MIN
    };
    let checks = [
        (
            p.ssim_mean >= SSIM_MEAN_MIN,
            format!("SSIM の平均 {:.4}", p.ssim_mean),
        ),
        (
            p.ssim_min >= frame_min,
            format!("SSIM の最小 {:.4}", p.ssim_min),
        ),
        (
            p.psnr_mean >= PSNR_MEAN_MIN,
            format!("PSNR {:.2}", p.psnr_mean),
        ),
        (s.snr_db >= SNR_MIN_DB, format!("音の SNR {:.1}", s.snr_db)),
        (
            s.envelope_db <= ENVELOPE_MAX_DB,
            format!("音の包絡の差 {:.2}", s.envelope_db),
        ),
        (
            s.lag_ms.abs() <= LAG_MAX_MS,
            format!("音のずれ {:.2} ms", s.lag_ms),
        ),
        (
            !full || s.gain_db.abs() <= GAIN_MAX_DB,
            format!("ラウドネスの差 {:.2}", s.gain_db),
        ),
        (
            s.peak_dbfs <= PEAK_MAX_DBFS,
            format!("音のピーク {:+.2} dBFS", s.peak_dbfs),
        ),
    ];
    checks
        .into_iter()
        .filter(|(ok, _)| !ok)
        .map(|(_, what)| format!("[{:.2}, {:.2}) {what}", range.0, range.1))
        .collect()
}

#[test]
fn ep03_を再現すると前の書き出しと一致する() {
    let Some(dir) = std::env::var_os(ENV_DIR).map(PathBuf::from) else {
        eprintln!("{ENV_DIR} が無いのでスキップする");
        return;
    };
    let expected = fixture();
    let (timeline, reference) = inputs(&dir, &expected);
    let work = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("作業のフォルダー");
    let project = work.path().join("project.json");
    let summary = import(&timeline, &project);
    check_gaps(&summary);
    let fps = expected["reference"]["fps"].as_f64().expect("fps");
    let frames = expected["reference"]["frames"].as_f64().expect("frames");
    let duration = summary["duration"].as_f64().expect("duration");
    assert_eq!(
        (duration * fps).round(),
        frames,
        "長さ（フレーム数）が前の書き出しと同じ"
    );
    let full = std::env::var(ENV_FULL).is_ok_and(|v| v == "1");
    let mut failed = Vec::new();
    for (i, range) in windows(duration, full).into_iter().enumerate() {
        let out = work.path().join(format!("w{i}.mp4"));
        render(&project, &out, (!full).then_some(range));
        let len = range.1 - range.0;
        let p = measure::picture(&out, &reference, range.0, len, work.path());
        assert_eq!(p.frames as f64, (len * fps).round(), "区間のフレーム数");
        let s = measure::sound(
            &measure::pcm(&out, 0.0, len),
            &measure::pcm(&reference, range.0, len),
            measure::peak_dbfs(&out),
        );
        report(range, &p, &s);
        failed.extend(misses(range, &p, &s, full));
    }
    assert!(failed.is_empty(), "閾値を割った: {failed:#?}");
}
