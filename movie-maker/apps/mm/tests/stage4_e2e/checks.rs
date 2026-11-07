//! 通しのテストの計測。書き出したファイルからフレームを抜いて SSIM で比べ、字幕・チャプター・ラウドネスを読む。

use std::path::Path;
use std::process::{Command, Output};

use mm_doc::Sequence;
use mm_eval::place_cuts;
use serde_json::Value;

/// 字幕の時刻の許容（秒）。字幕は ms に丸める。
pub const TIME_TOLERANCE: f64 = 0.02;

pub fn run(cmd: &mut Command) -> Output {
    let out = cmd.output().expect("コマンドを実行できる");
    assert!(
        out.status.success(),
        "{cmd:?} が失敗した: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// 書き出したファイルの `n` 番目のフレーム（0 始まり）を PNG に抜く。
pub fn extract_frame(video: &Path, n: i64, png: &Path) {
    run(Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-i"])
        .arg(video)
        .args(["-vf", &format!("select=eq(n\\,{n})")])
        .args(["-fps_mode", "passthrough", "-frames:v", "1"])
        .arg(png));
}

/// 2 枚の PNG の SSIM（全チャンネルの平均、1 が一致）。ffmpeg の `ssim` フィルター。
pub fn ssim(a: &Path, b: &Path) -> f64 {
    let out = run(Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(a)
        .arg("-i")
        .arg(b)
        .args(["-lavfi", "ssim", "-f", "null", "-"]));
    let log = String::from_utf8_lossy(&out.stderr).into_owned();
    let tail = log.rsplit("All:").next().expect("SSIM の行");
    tail.split_whitespace().next().unwrap().parse().unwrap()
}

/// 本編の PNG から、縦の画布に縦横比を保って収め、`scale` 倍して `shift` だけ横にずらして切り出した画像を作る。
/// レンダラーの再フレーミング（収める矩形の高さの丸め、画布の中心での拡大）を ffmpeg のフィルターで再現したもの。
pub fn reframe_png(
    main: &Path,
    out: &Path,
    canvas: (u32, u32),
    place_h: u32,
    scale: f64,
    shift: f64,
) {
    let (w, h) = (f64::from(canvas.0) * scale, f64::from(place_h) * scale);
    let (w, h) = (w.round() as i64, h.round() as i64);
    let x = (w - i64::from(canvas.0)) as f64 / 2.0 - shift;
    let y = (h - i64::from(canvas.1)) as f64 / 2.0;
    let filter = format!(
        "scale={}:{place_h}:flags=bilinear,scale={w}:{h}:flags=bilinear,crop={}:{}:{}:{}",
        canvas.0,
        canvas.0,
        canvas.1,
        x.round(),
        y.round()
    );
    run(Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-i"])
        .arg(main)
        .args(["-vf", &filter])
        .arg(out));
}

/// ffmpeg の ebur128 で測った統合ラウドネス（LUFS）とトゥルーピーク（dBFS）。
pub fn loudness(path: &Path) -> (f64, f64) {
    let out = run(Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-af", "ebur128=peak=true", "-vn", "-f", "null", "-"]));
    let log = String::from_utf8_lossy(&out.stderr).into_owned();
    let summary = log.rsplit("Summary:").next().unwrap();
    let value = |key: &str| -> f64 {
        let line = summary
            .lines()
            .find(|l| l.trim_start().starts_with(key))
            .unwrap();
        line.split_whitespace().nth(1).unwrap().parse().unwrap()
    };
    (value("I:"), value("Peak:"))
}

pub fn ffprobe_json(path: &Path, args: &[&str]) -> Value {
    let out = run(Command::new("ffprobe")
        .args(["-v", "error", "-of", "json"])
        .args(args)
        .arg(path));
    serde_json::from_slice(&out.stdout).unwrap()
}

/// 映像のフレームの数。
pub fn frame_count(path: &Path) -> i64 {
    let v = ffprobe_json(
        path,
        &[
            "-count_frames",
            "-select_streams",
            "v",
            "-show_entries",
            "stream=nb_read_frames",
        ],
    );
    v["streams"][0]["nb_read_frames"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

/// 字幕 1 件。
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

fn parse_time(s: &str) -> f64 {
    let (hms, ms) = s.trim().split_once([',', '.']).unwrap();
    let p: Vec<f64> = hms.split(':').map(|v| v.parse().unwrap()).collect();
    p[0] * 3600.0 + p[1] * 60.0 + p[2] + ms.parse::<f64>().unwrap() / 1000.0
}

pub fn parse_srt(text: &str) -> Vec<Cue> {
    text.replace("\r\n", "\n")
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .map(|block| {
            let mut lines = block.lines();
            lines.next().unwrap();
            let (a, b) = lines.next().unwrap().split_once("-->").unwrap();
            Cue {
                start: parse_time(a),
                end: parse_time(b),
                text: lines.collect::<Vec<_>>().join("\n"),
            }
        })
        .collect()
}

/// sequence のカットの（素材の in、素材の out、sequence の始まり）。素材の時刻から sequence の時刻を引く。
pub struct Timeline(Vec<(f64, f64, f64)>);

impl Timeline {
    pub fn of(seq: &Sequence) -> Self {
        let placed = place_cuts(seq).unwrap();
        Self(
            placed
                .iter()
                .map(|p| {
                    let cut = &seq.cuts[p.index];
                    (
                        cut.in_point.to_f64(),
                        cut.out_point.to_f64(),
                        p.range.start.to_f64(),
                    )
                })
                .collect(),
        )
    }

    /// 素材の時刻 `source` が sequence のどこに出るか。どのカットにも入らなければ `None`。
    pub fn at(&self, source: f64) -> Option<f64> {
        self.0
            .iter()
            .find(|(i, o, _)| source >= i - 1e-9 && source < o - 1e-9)
            .map(|(i, _, start)| start + source - i)
    }

    /// sequence の長さ（秒）。
    pub fn length(&self) -> f64 {
        self.0.iter().map(|(i, o, _)| o - i).sum()
    }
}

/// 字幕の先頭の単語の番号（「語N」）。
fn first_word_number(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('語')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// 字幕の始まりが、sequence に入る単語の始まりのどれかに合い、終わりが長さに収まり、字幕が重ならないこと。
/// 先頭が「語N」の字幕は、その語の素材の時刻から引いた位置に合うこと。`word_starts` は素材の単語の始まり（秒）。
/// 字幕の数と、その中で語の番号を確かめられた数を返す。
pub fn check_cues(cues: &[Cue], timeline: &Timeline, word_starts: &[f64]) -> (usize, usize) {
    assert!(cues.len() > 5, "字幕が少ない: {}", cues.len());
    let mut by_number = 0;
    for (i, c) in cues.iter().enumerate() {
        let hit = word_starts
            .iter()
            .filter_map(|s| timeline.at(*s))
            .any(|t| (t - c.start).abs() < TIME_TOLERANCE);
        assert!(
            hit,
            "字幕 {i}（{:?}）の始まり {} 秒に合う単語が無い",
            c.text, c.start
        );
        if let Some(n) = first_word_number(&c.text) {
            let want = timeline.at(word_starts[n]).expect("語がカットの中にある");
            assert!(
                (want - c.start).abs() < TIME_TOLERANCE,
                "字幕 {i}（語{n}）の始まり {} 秒（{want} 秒のはず）",
                c.start
            );
            by_number += 1;
        }
        assert!(c.end > c.start, "字幕 {i} の終わりが始まり以前");
        assert!(
            cues.get(i + 1).is_none_or(|n| c.end <= n.start + 1e-9),
            "字幕 {i} が次と重なる"
        );
    }
    let last = cues.last().unwrap();
    assert!(
        last.end <= timeline.length() + TIME_TOLERANCE,
        "最後の字幕の終わり {} 秒が長さ {} 秒を超える",
        last.end,
        timeline.length()
    );
    (cues.len(), by_number)
}
