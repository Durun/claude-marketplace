//! 組む前の文字列の下ごしらえ。ルビの注記（青空文庫の `｜親文字《ふりがな》`）を外して親文字だけにし、
//! 折り返してはいけない文字の間に WORD JOINER（U+2060）を挟む。cosmic-text は UAX #14 で折るので、
//! 親文字の中と、`LineBreak::Phrase` のときの文節の中を、この文字で 1 つの塊にする。
//! 組んだ後のグリフの位置は [`Prepared::source`] で元の文字列の位置に戻し、挟んだ文字のグリフは捨てる。

use mm_text_api::LineBreak;

/// 折り返しを禁じる文字。幅が無く、描かない。
const JOINER: char = '\u{2060}';
/// 親文字の始まりの印。
const BASE_MARK: char = '｜';
const RUBY_OPEN: char = '《';
const RUBY_CLOSE: char = '》';

/// 組む文字列の 1 文字と、元の文字列の中の位置。`source` が `None` なら挟んだ文字。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    out: usize,
    source: Option<(usize, usize)>,
}

/// 組む文字列と、元の文字列への対応。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prepared {
    /// cosmic-text に渡す文字列。
    pub text: String,
    spans: Vec<Span>,
    /// ルビがあったか。あれば `bounds` を上に広げる。
    pub has_ruby: bool,
}

impl Prepared {
    /// 組む文字列のバイト位置 `out` の文字の、元の文字列での位置と長さ。挟んだ文字なら `None`。
    pub fn source(&self, out: usize) -> Option<(usize, usize)> {
        let i = self
            .spans
            .partition_point(|s| s.out <= out)
            .checked_sub(1)?;
        self.spans[i].source
    }
}

/// 親文字だけを残した 1 文字。
#[derive(Debug, Clone, Copy)]
struct BaseChar {
    c: char,
    at: usize,
    /// 同じ親文字の次の文字とつなぐか。
    in_ruby: bool,
}

/// 親文字にしてよい文字（注記の `｜` が無いとき、`《` の直前の漢字の並び）。
fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' | '〆' | 'ヶ')
}

/// ルビの注記を外した文字の並び。閉じていない `《` はそのままの文字として残す。
fn strip_ruby(source: &str) -> (Vec<BaseChar>, bool) {
    let mut out: Vec<BaseChar> = Vec::new();
    let mut mark: Option<usize> = None;
    let mut has_ruby = false;
    let mut iter = source.char_indices().peekable();
    while let Some((at, c)) = iter.next() {
        if c == BASE_MARK {
            mark = Some(out.len());
            continue;
        }
        let closes = c == RUBY_OPEN && source[at..].contains(RUBY_CLOSE);
        if !closes {
            out.push(BaseChar {
                c,
                at,
                in_ruby: false,
            });
            continue;
        }
        let start = mark.take().unwrap_or_else(|| {
            let run = out.iter().rev().take_while(|b| is_kanji(b.c)).count();
            out.len() - run
        });
        for b in &mut out[start..] {
            b.in_ruby = true;
        }
        if let Some(last) = out.last_mut() {
            last.in_ruby = false;
        }
        has_ruby |= start < out.len();
        for (_, c) in iter.by_ref() {
            if c == RUBY_CLOSE {
                break;
            }
        }
    }
    (out, has_ruby)
}

/// 文節の中で次の文字とつなぐか（文字の番号ごと）。空白と改行の前後はつながない。
fn phrase_glue(chars: &[BaseChar]) -> Vec<bool> {
    let text: String = chars.iter().map(|b| b.c).collect();
    let model = budoux::models::default_japanese_model();
    let mut glue = Vec::with_capacity(chars.len());
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            glue.push(false);
        }
        for phrase in budoux::parse(model, line) {
            let len = phrase.chars().count();
            glue.extend((0..len).map(|i| i + 1 < len));
        }
    }
    glue.resize(chars.len(), false);
    for (i, g) in glue.iter_mut().enumerate() {
        let next = chars.get(i + 1).map(|b| b.c);
        *g &= !chars[i].c.is_whitespace() && !next.is_some_and(char::is_whitespace);
    }
    glue
}

/// 下ごしらえする。
pub(crate) fn prepare(source: &str, line_break: LineBreak) -> Prepared {
    let (chars, has_ruby) = strip_ruby(source);
    let phrase = match line_break {
        LineBreak::Phrase => phrase_glue(&chars),
        LineBreak::Anywhere => vec![false; chars.len()],
    };
    let mut text = String::with_capacity(source.len());
    let mut spans = Vec::with_capacity(chars.len());
    for (i, b) in chars.iter().enumerate() {
        spans.push(Span {
            out: text.len(),
            source: Some((b.at, b.c.len_utf8())),
        });
        text.push(b.c);
        if (b.in_ruby || phrase[i]) && i + 1 < chars.len() {
            spans.push(Span {
                out: text.len(),
                source: None,
            });
            text.push(JOINER);
        }
    }
    Prepared {
        text,
        spans,
        has_ruby,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(p: &Prepared) -> String {
        p.text.chars().filter(|c| *c != JOINER).collect()
    }

    #[test]
    fn ルビの注記を外して親文字をつなぐ() {
        let p = prepare("今日は｜東京《とうきょう》へ", LineBreak::Anywhere);
        assert_eq!(visible(&p), "今日は東京へ");
        assert_eq!(p.text, "今日は東\u{2060}京へ");
        assert!(p.has_ruby);
        let kyo = p.text.find('京').unwrap();
        let src = "今日は｜東京《とうきょう》へ";
        assert_eq!(p.source(kyo), Some((src.find('京').unwrap(), 3)));
        assert_eq!(p.source(kyo - 3), None, "挟んだ文字は元に無い");
    }

    #[test]
    fn 印が無ければ直前の漢字の並びが親文字() {
        let p = prepare("かな漢字《かんじ》です", LineBreak::Anywhere);
        assert_eq!(p.text, "かな漢\u{2060}字です");
    }

    #[test]
    fn 閉じていない括弧は文字のまま() {
        let p = prepare("《まだ", LineBreak::Anywhere);
        assert_eq!(p.text, "《まだ");
        assert!(!p.has_ruby);
    }

    #[test]
    fn 文節の中だけをつなぎ空白ではつながない() {
        let p = prepare("今日はいい天気です Hello world", LineBreak::Phrase);
        assert!(p.text.contains("今\u{2060}日"), "{:?}", p.text);
        let breaks = p
            .text
            .split(' ')
            .next()
            .unwrap()
            .chars()
            .filter(|c| *c != JOINER)
            .count()
            - p.text.split(' ').next().unwrap().matches(JOINER).count();
        assert!(breaks >= 2, "文節の切れ目が 1 つ以上残る: {:?}", p.text);
        assert!(!p.text.contains(" \u{2060}") && !p.text.contains("\u{2060} "));
        assert_eq!(visible(&p), "今日はいい天気です Hello world");
    }
}
