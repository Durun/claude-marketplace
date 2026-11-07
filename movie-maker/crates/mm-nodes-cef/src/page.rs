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
const PRELUDE: &str = "<script>(() => {\
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

/// `<head>` の直後（無ければ `<html>`、`<!doctype>` の直後、どれも無ければ先頭）に前置きを差し込む。
/// doctype より前に差し込むと Chromium が互換モードで組むので、先頭は最後の手段にする。
pub fn inject(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let at = ANCHORS
        .iter()
        .find_map(|tag| {
            let start = lower.find(tag)?;
            lower[start..].find('>').map(|end| start + end + 1)
        })
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
            out.starts_with("<!DOCTYPE html><html><HEAD><script>"),
            "{out}"
        );
        assert!(out.ends_with("<title>x</title></HEAD><body></body></html>"));
    }

    #[test]
    fn head_が無ければ_doctype_の後に入る() {
        let out = inject("<!doctype html><p>x</p>");
        assert!(out.starts_with("<!doctype html><script>"), "{out}");
    }

    #[test]
    fn 目印が無ければ先頭に入る() {
        assert!(inject("<p>x</p>").starts_with("<script>"));
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
