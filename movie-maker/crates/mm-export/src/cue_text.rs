//! 単語の並びを字幕の 1 件ずつに区切り、行に折る。時間の単位は ms。
//!
//! 決まり（YouTube 向けの横長の動画の字幕）。土台は Netflix の 2 つの規定:
//! - Japanese Timed Text Style Guide（<https://partnerhelp.netflixstudios.com/hc/en-us/articles/215767517-Japanese-Timed-Text-Style-Guide>）:
//!   横書きは 1 行に全角 13 字まで（半角は 0.5 字）、2 行まで、読む速さは 1 秒 4 字まで、1 件の最短 0.5 秒。最長の規定は無い。
//! - Timed Text Style Guide: General Requirements（<https://partnerhelp.netflixstudios.com/hc/en-us/articles/215758617-Timed-Text-Style-Guide-General-Requirements>）:
//!   1 件の最短 5/6 秒、最長 7 秒。
//!
//! この実装の値:
//! - 1 行は全角 20 字（半角は 0.5 字）まで、2 行まで。16:9 で字幕の文字を画面幅の半分ほどに収める YouTube では 13 字より
//!   20 字前後が読みやすいので伸ばす。
//! - 読む速さは 1 秒 4 字まで（Netflix の日本語の値）。足りない表示は次の字幕の始まりまで延ばし、7 秒で読み切れない量は
//!   溜めずに区切る。話す速さが上回る所は延ばす余地が無く、速いまま出る。
//! - 1 件の表示は 1 秒以上、7 秒以下。最長は General Requirements の値。最短は両方の規定より長めに取り、短い字幕は
//!   次の字幕の始まりまで延ばす。
//! - 文末（。！？）で区切る。0.7 秒以上の無音でも区切る（話が切れた所で字幕も切る）。読点で 1 行分たまったら区切る。
//! - 行の折り返しは、両方の行が 1 行に収まる位置のうち、2 行の長さが近く、読点や空白の後ろを優先する。行頭に閉じ括弧と
//!   句読点を置かない。

/// 1 行の最大の桁数。全角 1 字が 2 桁、半角 1 字が 1 桁。
pub const MAX_LINE_COLS: usize = 40;
/// 1 件の最大の行数。
pub const MAX_LINES: usize = 2;
/// 1 件の最短の表示（ms）。
pub const MIN_CUE_MS: i64 = 1_000;
/// 1 件の最長の表示（ms）。
pub const MAX_CUE_MS: i64 = 7_000;
/// 読む速さの上限（1 秒に読める全角の字数）。
pub const MAX_CHARS_PER_SECOND: usize = 4;
/// 全角 1 字の桁数。
const COLS_PER_CHAR: usize = 2;
const MS_PER_SECOND: usize = 1_000;
/// この長さ以上の無音は字幕を区切る（ms）。
pub const GAP_BREAK_MS: i64 = 700;
/// 折り返しで、読点や空白の後ろ以外に折るときの罰（桁）。
const BREAK_PENALTY: usize = 10;
/// 行頭に置けない文字を行頭に置く折り方の罰。
const KINSOKU_PENALTY: usize = 1_000;

/// 時刻つきの単語。
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// 区切った字幕の 1 件。`text` は折り返し済みで、行の区切りは `\n`。
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// 文字の桁数。
fn cols(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        2
    }
}

/// 文字列の桁数。
pub fn width(s: &str) -> usize {
    s.chars().map(cols).sum()
}

/// `text` を [`MAX_CHARS_PER_SECOND`] で読み切るのに要る表示（ms）。全角を 1 字、半角を 0.5 字に数え、改行は数えない。
pub fn reading_ms(text: &str) -> i64 {
    let cols = width(&text.replace('\n', ""));
    let cols_per_second = MAX_CHARS_PER_SECOND * COLS_PER_CHAR;
    let ms = (cols * MS_PER_SECOND).div_ceil(cols_per_second);
    i64::try_from(ms).unwrap_or(i64::MAX)
}

/// 2 行までに折れて、読む速さで最長の表示の内に読み切れるか。
fn fits_one_cue(text: &str) -> bool {
    reading_ms(text) <= MAX_CUE_MS && wrap(text).len() <= MAX_LINES
}

fn is_sentence_end(s: &str) -> bool {
    s.ends_with(['。', '！', '？', '!', '?', '．'])
}

fn is_clause_end(s: &str) -> bool {
    s.ends_with(['、', '，', ','])
}

/// 英数字どうしが隣り合うときだけ空白を挟む。日本語は詰める。
fn join(left: &str, right: &str) -> String {
    let alnum = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let space = alnum(left.chars().last()) && alnum(right.chars().next());
    format!("{left}{}{right}", if space { " " } else { "" })
}

/// 単語（時刻の順）を字幕に区切る。空の単語はスキップする。
pub fn segment(words: &[Word]) -> Vec<Cue> {
    let mut cues: Vec<Cue> = Vec::new();
    let mut cur: Option<Cue> = None;
    for w in words {
        let text = w.text.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(c) = cur.take() {
            let joined = join(&c.text, text);
            let must_break = w.start_ms - c.end_ms >= GAP_BREAK_MS
                || w.end_ms - c.start_ms > MAX_CUE_MS
                || !fits_one_cue(&joined);
            if must_break {
                cues.push(c);
                cur = Some(new_cue(w, text));
            } else {
                cur = Some(Cue {
                    start_ms: c.start_ms,
                    end_ms: w.end_ms.max(c.end_ms),
                    text: joined,
                });
            }
        } else {
            cur = Some(new_cue(w, text));
        }
        let ends = cur.as_ref().is_some_and(|c| {
            is_sentence_end(&c.text) || (is_clause_end(&c.text) && width(&c.text) >= MAX_LINE_COLS)
        });
        if ends {
            cues.extend(cur.take());
        }
    }
    cues.extend(cur);
    finish(cues)
}

fn new_cue(w: &Word, text: &str) -> Cue {
    Cue {
        start_ms: w.start_ms,
        end_ms: w.end_ms,
        text: text.to_owned(),
    }
}

/// 表示を最短と読む速さの分まで延ばし（最長の表示と次の件の始まりまで）、重なりを削り、行に折る。
fn finish(mut cues: Vec<Cue>) -> Vec<Cue> {
    for i in 0..cues.len() {
        let next_start = cues.get(i + 1).map(|c| c.start_ms);
        let c = &mut cues[i];
        let wanted = MIN_CUE_MS.max(reading_ms(&c.text)).min(MAX_CUE_MS);
        c.end_ms = c.end_ms.max(c.start_ms + wanted);
        if let Some(n) = next_start {
            c.end_ms = c.end_ms.min(n.max(c.start_ms + 1));
        }
        c.text = wrap(&c.text).join("\n");
    }
    cues
}

/// 行頭に置けない文字。
fn is_kinsoku_head(c: char) -> bool {
    "、。，．）」』】〕！？ー、。!?),.:;".contains(c)
}

/// 行末に置けない文字。
fn is_kinsoku_tail(c: char) -> bool {
    "（「『【〔(".contains(c)
}

/// `MAX_LINE_COLS` に収まるよう行に折る。両方が 1 行に収まる位置で 2 つに分ける。そういう位置が無いときだけ、
/// はみ出す側が少ない位置で分けて、分けた行を続けて折る。
pub fn wrap(text: &str) -> Vec<String> {
    if width(text) <= MAX_LINE_COLS {
        return vec![text.to_owned()];
    }
    let chars: Vec<char> = text.chars().collect();
    let total = width(text);
    // (はみ出す行の数, 罰, 位置) の最小を選ぶ。はみ出す行を先に比べるので、収まる位置があれば必ずそちらになる。
    let mut best: Option<(usize, usize, usize)> = None;
    let mut left = 0;
    for i in 1..chars.len() {
        left += cols(chars[i - 1]);
        let right = total - left;
        let space = chars[i - 1] == ' ' || chars[i] == ' ';
        let overflows = usize::from(left - usize::from(chars[i - 1] == ' ') > MAX_LINE_COLS)
            + usize::from(right - usize::from(chars[i] == ' ') > MAX_LINE_COLS);
        let ends_clause = space || is_clause_end(&chars[i - 1].to_string());
        let mut score = left.abs_diff(right) + if ends_clause { 0 } else { BREAK_PENALTY };
        if is_kinsoku_head(chars[i]) || is_kinsoku_tail(chars[i - 1]) {
            score += KINSOKU_PENALTY;
        }
        if best.is_none_or(|(o, s, _)| (overflows, score) < (o, s)) {
            best = Some((overflows, score, i));
        }
    }
    let at = best.map_or(chars.len() / 2, |(_, _, i)| i);
    let head: String = chars[..at].iter().collect();
    let tail: String = chars[at..].iter().collect();
    let mut lines = wrap(head.trim_end());
    lines.extend(wrap(tail.trim_start()));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(start: i64, end: i64, text: &str) -> Word {
        Word {
            start_ms: start,
            end_ms: end,
            text: text.into(),
        }
    }

    #[test]
    fn 文末で区切り_日本語は詰めて_英数字は空白を挟む() {
        let cues = segment(&[
            w(0, 300, "今日は"),
            w(300, 800, "Rust"),
            w(800, 1200, "です。"),
            w(1250, 1800, "次に"),
            w(1800, 2400, "進みます。"),
        ]);
        let texts: Vec<_> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["今日はRustです。", "次に進みます。"]);
    }

    #[test]
    fn 無音が続いたら区切る() {
        let cues = segment(&[w(0, 500, "あ"), w(500 + GAP_BREAK_MS, 2000, "い")]);
        assert_eq!(cues.len(), 2);
    }

    #[test]
    fn 短い字幕は次の字幕の手前まで延ばし_重ならない() {
        let cues = segment(&[
            w(0, 200, "あ。"),
            w(500, 700, "い。"),
            w(5000, 5200, "う。"),
        ]);
        assert_eq!(cues[0].end_ms, 500, "次の始まりで止まる");
        assert_eq!(cues[1].end_ms - cues[1].start_ms, MIN_CUE_MS);
        assert_eq!(cues[2].end_ms, 5000 + MIN_CUE_MS);
    }

    #[test]
    fn 長い一文は_2_行までに折り_1_行は_20_字以内() {
        let sentence = "これは書き出しの字幕の折り返しを確かめるための、少し長めの一文ですが、二行に収まります。";
        let words: Vec<_> = sentence
            .chars()
            .enumerate()
            .map(|(i, c)| w(i as i64 * 100, i as i64 * 100 + 100, &c.to_string()))
            .collect();
        let cues = segment(&words);
        for c in &cues {
            let lines: Vec<_> = c.text.lines().collect();
            assert!(lines.len() <= MAX_LINES, "{lines:?}");
            assert!(lines.iter().all(|l| width(l) <= MAX_LINE_COLS), "{lines:?}");
        }
        let joined: String = cues
            .iter()
            .flat_map(|c| c.text.chars())
            .filter(|c| *c != '\n')
            .collect();
        assert_eq!(joined, sentence, "字を欠かさない");
    }

    #[test]
    fn 折り返しは読点の後ろを選び_行頭に句読点を置かない() {
        let lines = wrap("これは長い前半の部分です、そして後半の部分が続いていきます。");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with('、'), "{lines:?}");
        let lines = wrap("あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほ。");
        assert!(!lines[1].starts_with('。'), "{lines:?}");
    }

    #[test]
    fn 片側が_1_行を超える割り方を選ばず_80_桁は_2_行に収める() {
        // 読点の後ろ（39 桁と 41 桁）は罰が無いが、41 桁の側が 1 行に収まらない。40 桁と 40 桁に分けられる。
        let text = format!("a{}、bb{}c", "あ".repeat(18), "い".repeat(19));
        assert_eq!(width(&text), MAX_LINE_COLS * MAX_LINES);
        let lines = wrap(&text);
        assert_eq!(lines.len(), MAX_LINES, "{lines:?}");
        assert!(lines.iter().all(|l| width(l) <= MAX_LINE_COLS), "{lines:?}");
    }

    #[test]
    fn 読む速さに足りない表示は次の字幕の手前まで延ばす() {
        // 12 字を 1 秒で話すと 3 秒要る。次の字幕は 5 秒から。
        let cues = segment(&[
            w(0, 1000, "あいうえおかきくけこさし。"),
            w(5000, 5500, "す。"),
        ]);
        let need = reading_ms("あいうえおかきくけこさし。");
        assert_eq!(cues[0].end_ms, need);
        // 次の字幕が近ければ、そこで止める（重ねない）。
        let cues = segment(&[
            w(0, 1000, "あいうえおかきくけこさし。"),
            w(1500, 2000, "す。"),
        ]);
        assert_eq!(cues[0].end_ms, 1500);
    }

    #[test]
    fn 読む速さで最長の表示を超える量は溜めずに区切る() {
        // 1 語 4 字を 0.5 秒ずつ。読点も文末も無く、無音も短い。
        let words: Vec<_> = (0..12)
            .map(|i| w(i * 500, i * 500 + 450, "あいうえ"))
            .collect();
        let cues = segment(&words);
        assert!(cues.len() >= 2);
        for c in &cues {
            assert!(reading_ms(&c.text.replace('\n', "")) <= MAX_CUE_MS, "{c:?}");
        }
    }

    #[test]
    fn 字数は全角を_1_字_半角を半分に数える() {
        assert_eq!(reading_ms("あいうえ"), 1_000);
        assert_eq!(reading_ms("abcdefgh"), 1_000);
    }

    #[test]
    fn 最長の表示を超えたら区切る() {
        let words: Vec<_> = (0..10).map(|i| w(i * 1000, i * 1000 + 900, "あ")).collect();
        let cues = segment(&words);
        assert!(cues.iter().all(|c| c.end_ms - c.start_ms <= MAX_CUE_MS));
        assert!(cues.len() >= 2);
    }
}
