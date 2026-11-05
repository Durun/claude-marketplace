//! 出力に要る最小の JSON の書き出し。serde を足さずに済む範囲だけ持つ。

use std::fmt::Write;

/// 文字列を JSON の文字列リテラルにする。
pub fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// 浮動小数。NaN と無限は JSON に無いので null にする。
pub fn number(out: &mut String, v: f64) {
    if v.is_finite() {
        let _ = write!(out, "{v}");
    } else {
        out.push_str("null");
    }
}

/// `"key":` を書く。
pub fn key(out: &mut String, k: &str) {
    string(out, k);
    out.push(':');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 制御文字と引用符を逃がす() {
        let mut s = String::new();
        string(&mut s, "a\"b\\c\n\u{1}");
        assert_eq!(s, r#""a\"b\\c\n\u0001""#);
    }
}
