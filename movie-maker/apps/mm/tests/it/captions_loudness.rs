//! 段階 4 の納品物の通し。37 秒の素材と合成の文字起こし（1 秒ごとの単語「語N」）から話題を 3 つ作り、順を入れ替えて並べ、
//! 1 つの話題の 3 語を `cutWords` で消して `mm render` で書き出す。確かめること:
//! - 字幕（SRT と WebVTT）の時刻がシーケンスの時間軸に合い、消した単語が入らず、行の決まりを守る。
//! - MP4 のチャプターが ffprobe で読め、YouTube の説明欄の文字列が出る。
//! - MP4 の字幕のトラック（mov_text）の中身が SRT と同じ。
//! - 書き出した音の統合ラウドネスが目標（YouTube -14、放送 -23）の ±0.5 LU に入り、トゥルーピークが -1 dBTP 以下。
//!
//! FFmpeg が要るので feature `ffmpeg` のときだけ実行する。

use std::path::Path;
use std::process::{Command, Output};

use mm_store::Store;
use mm_types::RationalTime;
use serde_json::{json, Value};

/// 素材の長さ（秒）と、単語の数（1 秒ごと）。
const MEDIA_SECONDS: u32 = 37;
const WORDS: i64 = 36;
/// 1 つの文の語数。最後の語に「。」を付ける。
const WORDS_PER_SENTENCE: i64 = 6;
/// 話題 A、B、C の単語の範囲（始まり、含まない終わり）。並べる順は C、A、B。チャプターは YouTube の決まりで
/// 10 秒以上なので、どの話題も（A は消した後も）11 秒にする。
const TOPICS: [(&str, i64, i64); 3] = [("tA", 0, 14), ("tB", 14, 25), ("tC", 25, 36)];
/// 消す単語（話題 A の中）。
const CUT: (i64, i64) = (5, 7);
/// 素材の音。小さな 1 kHz のトーンに、0.5 秒ごとの 2 ms の大きなパルスを足す。目標のラウドネスまで上げるとパルスが
/// トゥルーピークの上限を超えるので、リミッターが働く。
const PEAKY_TONE: &str = "0.08*sin(2*PI*1000*t)+0.7*lt(mod(t\\,0.5)\\,0.002)*sin(2*PI*3000*t)";
/// 時刻の許容（秒）。字幕は ms に丸める。
const TIME_TOLERANCE: f64 = 0.02;
/// 統合ラウドネスの許容（LU）。EBU R128 の許容。
const LUFS_TOLERANCE: f64 = 0.5;
/// トゥルーピークの上限（dBTP）。ffmpeg の測りと AAC の誤差の余裕を足す。
const PEAK_LIMIT: f64 = -0.9;
/// 書き出しのフレームレート。話題とカットの境（1 秒の倍数）がフレームの境に乗る最小の値。確かめるのは字幕、
/// チャプター、ラウドネスで、絵の細かさに依らない。CI の GPU は WARP（ソフトウェア）でフレームごとに重いので低くする。
const FPS: u32 = 1;

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

/// 単語 `i` の始まり（秒）。1 秒おき。
fn word_start(i: i64) -> f64 {
    i as f64
}

fn make_media(out: &Path) {
    let seconds = MEDIA_SECONDS.to_string();
    run(Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=320x180:r=30:d={seconds}"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!(
            "aevalsrc=exprs='{PEAKY_TONE}':sample_rate=48000:duration={seconds}"
        ))
        .args(["-t", &seconds, "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac"])
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
                .map(|i| {
                    let mark = if i % WORDS_PER_SENTENCE == WORDS_PER_SENTENCE - 1 {
                        "。"
                    } else {
                        ""
                    };
                    // 単語は 0.5 秒で終わり、次の単語まで 0.5 秒の隙間を空ける。
                    json!({"start": rt(i, 1), "end": rt(2 * i + 1, 2), "text": format!("語{i}{mark}")})
                })
                .collect();
            json!({"start": rt(range.start, 1), "end": rt(2 * (range.end - 1) + 1, 2),
                "text": "", "words": words})
        })
        .collect();
    let sidecar = json!({"schema_version": 1, "content_hash": hash, "producer": "test",
        "kind": "transcript", "language": "ja", "segments": segments});
    let path = mm_ingest::CacheLayout::new(dir.join("cache")).sidecar(&hash, "transcript");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, sidecar.to_string()).unwrap();
}

/// 話題を C、A、B の順に並べた sequence `s1` を作り、A の 3 語を消す。
fn build_sequence(dir: &str, asset: &str, word_ids: &[String]) {
    for (id, from, to) in TOPICS {
        op(
            dir,
            json!({"op": "addTopic", "topic": {"id": id, "asset": asset,
            "in": rt(from, 1), "out": rt(to, 1), "title": id}}),
        );
    }
    op(
        dir,
        json!({"op": "addSequence", "sequence": {"id": "s1", "cuts": []}}),
    );
    op(
        dir,
        json!({"op": "insertTopics", "sequence": "s1", "topics": ["tC", "tA", "tB"],
        "cuts": ["k1", "k2", "k3"]}),
    );
    let picked: Vec<_> = (CUT.0..=CUT.1).map(|i| &word_ids[i as usize]).collect();
    op(
        dir,
        json!({"op": "cutWords", "asset": asset, "sequence": "s1", "words": picked,
        "ranges": [{"in": rt(CUT.0, 1), "out": rt(CUT.1 + 1, 1)}]}),
    );
}

/// 消した区間の長さ（秒）。消した最後の語の後の隙間まで含む。
fn removed() -> f64 {
    word_start(CUT.1 + 1) - word_start(CUT.0)
}

/// 書き出しの時間軸での単語 `n` の始まり（秒）。並べる順は C、A、B で、A から 3 語が抜けている。
fn timeline_start(n: i64) -> f64 {
    let [(_, a0, a1), (_, b0, _), (_, c0, c1)] = TOPICS;
    let len_c = word_start(c1) - word_start(c0);
    let len_a = word_start(a1) - word_start(a0) - removed();
    match n {
        n if n >= c0 => word_start(n) - word_start(c0),
        n if n < CUT.0 => len_c + word_start(n),
        n if n < a1 => len_c + word_start(n) - removed(),
        _ => len_c + len_a + word_start(n) - word_start(b0),
    }
}

/// 書き出しの全長（秒）。
fn timeline_length() -> f64 {
    TOPICS
        .iter()
        .map(|(_, from, to)| word_start(*to) - word_start(*from))
        .sum::<f64>()
        - removed()
}

fn add_output(dir: &str) {
    let length = timeline_length();
    let source = json!({"kind": "sequence", "version": 1,
        "params": {"sequence": {"value": {"type": "text", "value": "s1"}}}});
    let layer = json!({"id": "l1", "source": source, "start": rt(0, 1), "in": rt(0, 1),
        "out": rt((length * 4.0) as i64, 4),
        "opacity": {"value": {"type": "number", "value": 1.0}}});
    op(
        dir,
        json!({"op": "addComposition", "composition": {"id": "main", "width": 320,
        "height": 180, "fps": {"num": FPS, "den": 1}, "duration": rt((length * 4.0) as i64, 4),
        "layers": [layer]}}),
    );
    let marks = [
        ("m1", 0.0, "導入"),
        ("m2", timeline_start(0), "本題"),
        ("m3", timeline_start(TOPICS[1].1), "まとめ"),
    ];
    for (id, at, name) in marks {
        op(
            dir,
            json!({"op": "addMarker", "composition": "main", "marker":
            {"id": id, "time": rt((at * 4.0) as i64, 4), "duration": rt(0, 1), "name": name}}),
        );
    }
    let target = |id: &str, lufs: f64, delivery: Value| {
        json!({"op": "addRenderTarget", "target": {"id": id, "composition": "main",
            "width": 320, "height": 180, "fps": {"num": FPS, "den": 1}, "codec": "h264",
            "output_path": format!("{id}.mp4"), "loudness_lufs": lufs, "delivery": delivery}})
    };
    op(
        dir,
        target(
            "yt",
            -14.0,
            json!({"subtitles": "sidecar", "chapters": true}),
        ),
    );
    op(dir, target("tv", -23.0, json!({"subtitles": "embedded"})));
}

/// 字幕 1 件。
struct Cue {
    start: f64,
    end: f64,
    text: String,
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

/// 字幕の文字の中の単語の番号（「語N」）。
fn word_numbers(text: &str) -> Vec<i64> {
    text.split('語')
        .skip(1)
        .map(|s| {
            let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap()
        })
        .collect()
}

fn check_cues(cues: &[Cue]) {
    // 1 文に 1 つ以上の字幕が出る。
    assert!(
        cues.len() as i64 >= WORDS / WORDS_PER_SENTENCE,
        "字幕が少ない: {}",
        cues.len()
    );
    let mut seen = Vec::new();
    for (i, c) in cues.iter().enumerate() {
        let first = word_numbers(&c.text)[0];
        assert!(
            (c.start - timeline_start(first)).abs() < TIME_TOLERANCE,
            "字幕 {i}（語{first}）の始まり {} 秒（{} 秒のはず）",
            c.start,
            timeline_start(first)
        );
        assert!(c.end > c.start && cues.get(i + 1).is_none_or(|n| c.end <= n.start + 1e-9));
        let lines: Vec<_> = c.text.lines().collect();
        assert!(lines.len() <= 2, "3 行以上: {:?}", c.text);
        let width = |l: &str| {
            l.chars()
                .map(|c| if c.is_ascii() { 1 } else { 2 })
                .sum::<usize>()
        };
        assert!(
            lines.iter().all(|l| width(l) <= 40),
            "1 行が 20 字を超える: {:?}",
            c.text
        );
        seen.extend(word_numbers(&c.text));
    }
    assert!(
        seen.windows(2).all(|w| w[0] != w[1]),
        "同じ単語が二重に出る"
    );
    for n in CUT.0..=CUT.1 {
        assert!(!seen.contains(&n), "消した語{n} が字幕に入っている");
    }
    let expected = WORDS - (CUT.1 - CUT.0 + 1);
    assert_eq!(seen.len() as i64, expected, "消した以外の語が全部出る");
}

/// ffmpeg の ebur128 で測った統合ラウドネス（LUFS）とトゥルーピーク（dBFS）。
fn loudness(path: &Path) -> (f64, f64) {
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

fn ffprobe_json(path: &Path, args: &[&str]) -> Value {
    let out = run(Command::new("ffprobe")
        .args(["-v", "error", "-of", "json"])
        .args(args)
        .arg(path));
    serde_json::from_slice(&out.stdout).unwrap()
}

/// ストリーム（`v` か `a`）の始まりの時刻（秒）。
fn stream_start(path: &Path, kind: &str) -> f64 {
    ffprobe_json(
        path,
        &[
            "-select_streams",
            kind,
            "-show_entries",
            "stream=start_time",
        ],
    )["streams"][0]["start_time"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn 字幕とチャプターとラウドネスが書き出しの時間軸と目標に合う() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let media = tmp.path().join("talk.mp4");
    make_media(&media);
    let d = dir.to_str().unwrap();
    mm(&["new", d]);
    place_transcript(&dir, &media);
    mm(&["ingest", d, media.to_str().unwrap(), "--encoder", "x264"]);
    let store = Store::open_read_only(&dir).unwrap();
    let asset = store.project().assets[0].id.clone();
    let ids: Vec<String> = store
        .transcript_words(&asset)
        .unwrap()
        .iter()
        .map(|w| w.word.to_string())
        .collect();
    drop(store);
    build_sequence(d, asset.as_str(), &ids);
    add_output(d);

    let yt = tmp.path().join("yt.mp4");
    let tv = tmp.path().join("tv.mp4");
    mm(&["render", d, "--target", "yt", "-o", yt.to_str().unwrap()]);
    mm(&["render", d, "--target", "tv", "-o", tv.to_str().unwrap()]);

    let srt = parse_srt(&std::fs::read_to_string(yt.with_extension("srt")).unwrap());
    check_cues(&srt);
    let vtt = std::fs::read_to_string(yt.with_extension("vtt")).unwrap();
    assert!(vtt.starts_with("WEBVTT\n"));
    assert_eq!(vtt.matches(" --> ").count(), srt.len());

    let chapters = ffprobe_json(&yt, &["-show_chapters"]);
    let chapters = chapters["chapters"].as_array().unwrap();
    assert_eq!(chapters.len(), 3, "{chapters:?}");
    // チャプターは映像と同じ時間軸に置く。映像の始まりがずれていれば、同じだけずれていないと話題の先頭と合わない。
    let video_start = stream_start(&yt, "v");
    let starts = [0.0, timeline_start(0), timeline_start(TOPICS[1].1)];
    for (c, (start, title)) in chapters
        .iter()
        .zip(starts.iter().zip(["導入", "本題", "まとめ"]))
    {
        let at: f64 = c["start_time"].as_str().unwrap().parse().unwrap();
        assert!(
            (at - video_start - start).abs() < TIME_TOLERANCE,
            "チャプター {title} の始まり {at}（映像の始まり {video_start}）"
        );
        assert_eq!(c["tags"]["title"], title);
    }
    let text = std::fs::read_to_string(yt.with_extension("chapters.txt")).unwrap();
    assert_eq!(text, "00:00 導入\n00:11 本題\n00:22 まとめ\n");

    let tracks = ffprobe_json(&tv, &["-show_entries", "stream=codec_name,codec_type"]);
    let codecs: Vec<_> = tracks["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["codec_name"].as_str().unwrap().to_owned())
        .collect();
    assert!(codecs.contains(&"mov_text".to_owned()), "{codecs:?}");
    let extracted = tmp.path().join("tv.srt");
    run(Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-i"])
        .arg(&tv)
        .args(["-map", "0:s:0"])
        .arg(&extracted));
    // 映像の始まりがずれていても字幕が同じだけずれていればよいので、映像の始まりを引いて比べる。
    let shift = stream_start(&tv, "v");
    let embedded = parse_srt(&std::fs::read_to_string(&extracted).unwrap());
    assert_eq!(embedded.len(), srt.len(), "トラックの字幕の数が SRT と違う");
    for (a, b) in embedded.iter().zip(&srt) {
        assert!(
            (a.start - shift - b.start).abs() < TIME_TOLERANCE && a.text == b.text,
            "トラックの字幕 {} 秒 {:?} が SRT の {} 秒 {:?} と違う",
            a.start,
            a.text,
            b.start,
            b.text
        );
    }

    for (path, target) in [(&yt, -14.0), (&tv, -23.0)] {
        let (lufs, peak) = loudness(path);
        eprintln!(
            "{}: 統合ラウドネス {lufs:.1} LUFS（目標 {target}）、ピーク {peak:.1} dBFS",
            path.display()
        );
        assert!(
            (lufs - target).abs() <= LUFS_TOLERANCE,
            "{lufs} LUFS（{target} のはず）"
        );
        assert!(peak <= PEAK_LIMIT, "{peak} dBFS");
    }
}
