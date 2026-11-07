//! ページが読み込んでよい URL の決まり（CEF の要らない検査）。

use mm_host_cef::policy::{allows_navigation, allows_resource};

const PAGE: &str = "file:///C:/Users/a/AppData/Local/Temp/mm-host-cef-1-0/pages/page-0.html";

#[test]
fn ページそのものと埋め込みの資源だけを読み込める() {
    assert!(allows_resource(PAGE, PAGE));
    assert!(allows_resource("data:font/woff2;base64,AAAA", PAGE));
    assert!(allows_resource("DATA:image/png;base64,AAAA", PAGE));
    assert!(allows_resource(
        "blob:file:///6f1c1f2e-0000-4000-8000-000000000000",
        PAGE
    ));
}

#[test]
fn ネットワークとほかのファイルは読み込めない() {
    for url in [
        "http://127.0.0.1:8080/a.png",
        "https://example.com/font.woff2",
        "ws://127.0.0.1/",
        "file:///C:/Windows/win.ini",
        "file:///C:/Users/a/AppData/Local/Temp/mm-host-cef-1-0/pages/page-1.html",
        "chrome://version",
        "",
    ] {
        assert!(!allows_resource(url, PAGE), "{url}");
    }
}

#[test]
fn 遷移はページそのものだけ() {
    assert!(allows_navigation(PAGE, PAGE));
    for url in [
        "http://127.0.0.1/",
        "data:text/html,<p>x</p>",
        "file:///C:/Windows/win.ini",
        "about:blank",
    ] {
        assert!(!allows_navigation(url, PAGE), "{url}");
    }
    assert!(!allows_navigation("", ""), "ページが無ければ何も通さない");
}
