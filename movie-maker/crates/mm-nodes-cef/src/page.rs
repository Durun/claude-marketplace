//! パラメータからページを読む。ページの HTML には、時刻と倍率を受ける前置きのスクリプトを差し込む。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use mm_types::{ParamValue, Params, RationalTime, Size};

/// パラメータの名前: ページの HTML。
pub const HTML_PARAM: &str = "html";
/// パラメータの名前: ページに渡す値（JSON）。
pub const DATA_PARAM: &str = "data";
/// パラメータの名前: 真なら時刻によらず 1 枚だけ描く。
pub const STATIC_PARAM: &str = "static";

/// ページの `window.mmSetTime(t, data)` を、ホストが渡す `(t, {data, scale})` から呼ぶ前置き。
/// 倍率は CSS の `zoom` で掛けるので、ページは 1 倍の画素で組めばプレビューの縮小でも同じ見た目になる。
/// 先頭の CSP は、パックやプロジェクトの HTML がネットワークとローカルのファイルを読まないようにする
/// （外の資源は画素を決定的でなくし、ファイルの中身を画面に出せてしまう）。資源は `data:` と `blob:` で埋め込む。
const PRELUDE: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data: blob:; \
font-src data:; media-src data: blob:; base-uri 'none'; form-action 'none'\">\
<script>(() => {\
let user = null;\
Object.defineProperty(window, 'mmSetTime', { configurable: true,\
get() { return (t, p) => { const q = p || {};\
document.documentElement.style.zoom = String(q.scale || 1);\
return user ? user(t, q.data || {}) : undefined; }; },\
set(f) { user = f; } });\
})();</script>";

/// 前置きを差し込む位置の目印。前にあるものほど優先する。
const ANCHORS: [&str; 3] = ["<head", "<html", "<!doctype"];

/// 1 枚のページの入力。
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// 前置きを差し込んだ HTML。
    pub html: String,
    /// `html` のハッシュ。ホストとキャッシュの照合に使う。
    pub hash: u64,
    /// ページに渡す値。
    pub data: serde_json::Value,
    /// `data` の JSON の文字列（キーは名前の順）。キャッシュのキーに使う。
    pub data_text: String,
    /// 時刻によらず 1 枚だけ描くか。
    pub is_static: bool,
}

impl Page {
    /// パラメータから読む。HTML が空なら `None`（何も描かない）。
    pub fn read(params: &Params) -> Option<Self> {
        let raw = params.get(HTML_PARAM).and_then(ParamValue::as_text)?;
        if raw.trim().is_empty() {
            return None;
        }
        let data = match params.get(DATA_PARAM) {
            Some(ParamValue::Json(v)) => v.clone(),
            _ => serde_json::Value::Object(Default::default()),
        };
        let html = inject(raw);
        Some(Self {
            hash: hash_of(&html),
            data_text: data.to_string(),
            data,
            html,
            is_static: params
                .get(STATIC_PARAM)
                .and_then(ParamValue::as_bool)
                .unwrap_or(false),
        })
    }

    /// ページに渡す時刻。静止のページは 0 に揃え、同じパラメータなら同じ 1 枚にする。
    pub fn time(&self, source_time: RationalTime) -> RationalTime {
        if self.is_static {
            RationalTime::ZERO
        } else {
            source_time
        }
    }

    /// ホストの `RenderFrame` の `params`。
    pub fn frame_params(&self, scale: f32) -> serde_json::Value {
        serde_json::json!({ "data": self.data, "scale": scale })
    }
}

/// キャッシュとホストの照合に使うハッシュ。プロセスの中だけで使うので標準のハッシュで足りる。
pub fn hash_of(value: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

/// 最初の `tag` の開きタグの直後の位置。`<head` が `<header>` に当たらないよう、名前の直後の文字も見る。
fn tag_end(lower: &str, tag: &str) -> Option<usize> {
    lower.match_indices(tag).find_map(|(start, _)| {
        let rest = &lower[start + tag.len()..];
        let ends_name = rest
            .chars()
            .next()
            .is_some_and(|c| c == '>' || c == '/' || c.is_ascii_whitespace());
        ends_name
            .then(|| rest.find('>'))
            .flatten()
            .map(|end| start + tag.len() + end + 1)
    })
}

/// `<head>` の直後（無ければ `<html>`、`<!doctype>` の直後、どれも無ければ先頭）に前置きを差し込む。
/// doctype より前に差し込むと Chromium が互換モードで組むので、先頭は最後の手段にする。
pub fn inject(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let at = ANCHORS
        .iter()
        .find_map(|tag| tag_end(&lower, tag))
        .unwrap_or(0);
    let mut out = String::with_capacity(html.len() + PRELUDE.len());
    out.push_str(&html[..at]);
    out.push_str(PRELUDE);
    out.push_str(&html[at..]);
    out
}

/// 1 フレームの描画の頼み。
#[derive(Debug, Clone, Copy)]
pub struct FrameRequest<'a> {
    pub page: &'a Page,
    pub time: RationalTime,
    pub size: Size,
    pub scale: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 前置きは_head_の直後に入る() {
        let html = "<!DOCTYPE html><html><HEAD><title>x</title></HEAD><body></body></html>";
        let out = inject(html);
        assert!(
            out.starts_with(&format!("<!DOCTYPE html><html><HEAD>{PRELUDE}")),
            "{out}"
        );
        assert!(out.ends_with("<title>x</title></HEAD><body></body></html>"));
    }

    #[test]
    fn head_が無ければ_doctype_の後に入る() {
        let out = inject("<!doctype html><p>x</p>");
        assert!(
            out.starts_with(&format!("<!doctype html>{PRELUDE}")),
            "{out}"
        );
    }

    #[test]
    fn header_の要素は_head_と取り違えない() {
        let out = inject("<!doctype html><html><body><header>x</header></body></html>");
        assert!(
            out.starts_with(&format!("<!doctype html><html>{PRELUDE}")),
            "{out}"
        );
        let out = inject("<html><head lang=\"ja\"><title>x</title></head></html>");
        assert!(
            out.starts_with(&format!("<html><head lang=\"ja\">{PRELUDE}")),
            "{out}"
        );
    }

    #[test]
    fn 目印が無ければ先頭に入る() {
        assert!(inject("<p>x</p>").starts_with(PRELUDE));
    }

    #[test]
    fn 前置きはネットワークとファイルの読み込みを禁じる() {
        assert!(PRELUDE.starts_with("<meta http-equiv=\"Content-Security-Policy\""));
        assert!(PRELUDE.contains("default-src 'none'"));
        assert!(!PRELUDE.contains("http:") && !PRELUDE.contains("file:"));
    }

    #[test]
    fn 空の_html_はページにしない() {
        let mut p = Params::new();
        p.set(HTML_PARAM, ParamValue::Text("  ".into()));
        assert_eq!(Page::read(&p), None);
    }

    #[test]
    fn 静止のページは時刻を_0_に揃える() {
        let mut p = Params::new();
        p.set(HTML_PARAM, ParamValue::Text("<p>x</p>".into()))
            .set(STATIC_PARAM, ParamValue::Bool(true));
        let page = Page::read(&p).unwrap();
        assert_eq!(page.time(RationalTime::seconds(3)), RationalTime::ZERO);
    }
}
