//! スパンの名前の決まり。`<クレート>.<動詞>`（`render.frame`、`decode.seek`、`ops.apply`）。

/// 区切り。
pub const SEPARATOR: char = '.';

/// `<クレート>.<動詞>` の形か。各部は英小文字、数字、`_` だけ。
pub fn is_valid_span_name(name: &str) -> bool {
    let mut parts = name.split(SEPARATOR);
    let (Some(crate_part), Some(verb_part), None) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    is_ident(crate_part) && is_ident(verb_part)
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn クレートと動詞の二段だけ通す() {
        assert!(is_valid_span_name("render.frame"));
        assert!(is_valid_span_name("decode.seek_to"));
        assert!(!is_valid_span_name("render"));
        assert!(!is_valid_span_name("render.frame.layer"));
        assert!(!is_valid_span_name("Render.Frame"));
        assert!(!is_valid_span_name("render."));
    }
}
