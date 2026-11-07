//! 組む前の文字列の前処理。ルビの注記（[`mm_text_api::parse_ruby`]）を外して親文字だけにし、
//! 折り返してはいけない文字の間に WORD JOINER（U+2060）を挟む。cosmic-text は UAX #14 で折るので、
//! 親文字の中と、`LineBreak::Phrase` のときの文節の中を、この文字で 1 つの塊にする。
//! 組んだ後のグリフの位置は [`Prepared::source`] で元の文字列の位置に戻し、挟んだ文字のグリフは捨てる。

use mm_text_api::{has_ruby, parse_ruby, LineBreak};

/// 折り返しを禁じる文字。幅が無く、描かない。
const JOINER: char = '\u{2060}';

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

    /// 組む文字列の範囲 `start..end` の中で、最初に元の文字列にある文字の位置と長さ。
    /// 挟んだ文字から始まるクラスターも、続く文字の位置に戻す。
    pub fn first_source(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let from = self.spans.partition_point(|s| s.out < start);
        let found = self.spans[from..]
            .iter()
            .take_while(|s| s.out < end)
            .find_map(|s| s.source);
        found.or_else(|| self.source(start))
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

/// ルビの注記を外した文字の並びと、ルビがあったか。
fn strip_ruby(source: &str) -> (Vec<BaseChar>, bool) {
    let spans = parse_ruby(source);
    let mut out = Vec::with_capacity(source.len());
    for span in &spans {
        let base = &source[span.base.clone()];
        let count = base.chars().count();
        out.extend(base.char_indices().enumerate().map(|(n, (i, c))| BaseChar {
            c,
            at: span.base.start + i,
            in_ruby: span.ruby.is_some() && n + 1 < count,
        }));
    }
    (out, has_ruby(&spans))
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

/// 前処理する。
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
        assert_eq!(
            p.first_source(kyo - 3, kyo + 3),
            Some((src.find('京').unwrap(), 3)),
            "挟んだ文字から始まるクラスターは続く文字に戻す"
        );
    }

    #[test]
    fn 縦線が無ければ直前の漢字の並びが親文字() {
        let p = prepare("かな漢字《かんじ》です", LineBreak::Anywhere);
        assert_eq!(p.text, "かな漢\u{2060}字です");
    }

    #[test]
    fn 親文字にならない注記の記号は文字のまま() {
        for text in ["《まだ", "見て《ほしい》", "A｜B"] {
            let p = prepare(text, LineBreak::Anywhere);
            assert_eq!(p.text, text);
            assert!(!p.has_ruby);
        }
    }

    #[test]
    fn 文節の中だけをつなぎ空白ではつながない() {
        let p = prepare("今日はいい天気です Hello world", LineBreak::Phrase);
        assert!(p.text.contains("今\u{2060}日"), "{:?}", p.text);
        let head = p.text.split(' ').next().unwrap();
        let chars = head.chars().filter(|c| *c != JOINER).count();
        let joins = head.matches(JOINER).count();
        assert!(
            joins + 1 < chars,
            "文節の切れ目が 1 つ以上残る: {:?}",
            p.text
        );
        assert!(!p.text.contains(" \u{2060}") && !p.text.contains("\u{2060} "));
        assert_eq!(visible(&p), "今日はいい天気です Hello world");
    }
}
