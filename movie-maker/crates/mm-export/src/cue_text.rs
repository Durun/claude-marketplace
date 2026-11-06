//! 単語の列を字幕の 1 件ずつに区切り、行に折る。時間の単位は ms。
//!
//! 決まり（YouTube 向けの横長の動画の字幕。Netflix の日本語の字幕の規定を土台にして、画面が広い分だけ 1 行を長くした）:
//! - 1 行は全角 20 字（半角は 0.5 字）まで、2 行まで。Netflix の日本語は 1 行 13 字だが、16:9 で字幕の文字を画面幅の
//!   半分以内に収める YouTube では 20 字前後が読みやすい。
//! - 1 件の表示は 1 秒以上、7 秒以下（Netflix の最短 5/6 秒、最長 7 秒）。短い字幕は次の字幕の始まりまで延ばす。
//! - 文末（。！？）で区切る。0.7 秒以上の無音でも区切る（話が切れた所で字幕も切る）。読点で 1 行分たまったら区切る。
//! - 行の折り返しは、2 行の長さが近くなる位置のうち、読点や空白の後ろを優先する。行頭に閉じ括弧と句読点を置かない。

/// 1 行の最大の桁数。全角 1 字が 2 桁、半角 1 字が 1 桁。
pub const MAX_LINE_COLS: usize = 40;
/// 1 件の最大の行数。
pub const MAX_LINES: usize = 2;
/// 1 件の最短の表示（ms）。
pub const MIN_CUE_MS: i64 = 1_000;
/// 1 件の最長の表示（ms）。
pub const MAX_CUE_MS: i64 = 7_000;
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

/// 単語（時刻の順）を字幕に区切る。空の単語は読み飛ばす。
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
                || width(&joined) > MAX_LINE_COLS * MAX_LINES
                || w.end_ms - c.start_ms > MAX_CUE_MS;
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

/// 表示を最短まで延ばし（次の件の始まりまで）、重なりを削り、行に折る。
fn finish(mut cues: Vec<Cue>) -> Vec<Cue> {
    for i in 0..cues.len() {
        let next_start = cues.get(i + 1).map(|c| c.start_ms);
        let c = &mut cues[i];
        c.end_ms = c.end_ms.max(c.start_ms + MIN_CUE_MS);
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

/// `MAX_LINE_COLS` に収まるよう行に折る。2 つに分けて、まだ長ければ分けた行を続けて折る。
pub fn wrap(text: &str) -> Vec<String> {
    if width(text) <= MAX_LINE_COLS {
        return vec![text.to_owned()];
    }
    let chars: Vec<char> = text.chars().collect();
    let total = width(text);
    let mut best: Option<(usize, usize)> = None;
    let mut left = 0;
    for i in 1..chars.len() {
        left += cols(chars[i - 1]);
        let right = total - left;
        let balance = left.abs_diff(right);
        let ends_clause = chars[i - 1] == ' ' || is_clause_end(&chars[i - 1].to_string());
        let mut score = balance + if ends_clause { 0 } else { BREAK_PENALTY };
        if is_kinsoku_head(chars[i]) || is_kinsoku_tail(chars[i - 1]) {
            score += KINSOKU_PENALTY;
        }
        if left > MAX_LINE_COLS && right > MAX_LINE_COLS {
            score += KINSOKU_PENALTY;
        }
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, i));
        }
    }
    let at = best.map_or(chars.len() / 2, |(_, i)| i);
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
        assert_eq!(joined, sentence, "字を落とさない");
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
    fn 最長の表示を超えたら区切る() {
        let words: Vec<_> = (0..10).map(|i| w(i * 1000, i * 1000 + 900, "あ")).collect();
        let cues = segment(&words);
        assert!(cues.iter().all(|c| c.end_ms - c.start_ms <= MAX_CUE_MS));
        assert!(cues.len() >= 2);
    }
}
