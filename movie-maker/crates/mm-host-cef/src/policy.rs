//! ページが読み込んでよい URL の決まり。ホストの `RequestHandler` がこれで遷移と要求を拒否する。
//!
//! ページはパックやプロジェクトの HTML で、他人が書いたものでもある。ネットワークとファイルを読めると、
//! 画素が決定的でなくなり、ローカルのファイルの中身を画面に出せてしまう。そこで、ホストが書いたページそのものと、
//! ページの中に埋め込んだ資源（`data:` と `blob:`）だけを許す。

/// ページの中に埋め込める資源の scheme。
const EMBEDDED_SCHEMES: [&str; 2] = ["data:", "blob:"];

/// `url` の scheme が `scheme`（`data:` の形）か。大文字と小文字を区別しない。
fn has_scheme(url: &str, scheme: &str) -> bool {
    url.get(..scheme.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
}

/// ページ `page` の中の資源の要求 `url` を通すか。ページそのものと、`data:` と `blob:` だけを通す。
pub fn allows_resource(url: &str, page: &str) -> bool {
    url == page || EMBEDDED_SCHEMES.iter().any(|s| has_scheme(url, s))
}

/// ページ `page` を開いたブラウザーの遷移 `url` を通すか。ホストが開いたページそのものだけを通す
/// （`location` の書き換え、リンク、フォームの送信、`data:` への遷移を止める）。
pub fn allows_navigation(url: &str, page: &str) -> bool {
    !page.is_empty() && url == page
}
