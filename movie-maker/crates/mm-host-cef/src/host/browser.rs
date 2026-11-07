//! ブラウザの作成と、時刻を決めて 1 フレームを受け取る手順（bench と serve で共有）。
//!
//! 1 フレームの手順:
//! 1. ページの JS で時刻を反映し、`document.title` で反映済みを知らせてもらう（JS と BeginFrame の順は IPC で
//!    保証されないので、この往復で順序を作る）。
//! 2. `send_external_begin_frame` を送り、フレームが来なければ送り直す（BeginFrame は捨てられることがある）。

use std::rc::Rc;
use std::time::{Duration, Instant};

use cef::*;

use super::handlers::{build_client, Shared};

/// 読み込みや時刻の反映を待つ上限。
pub const WAIT_LIMIT: Duration = Duration::from_secs(5);

/// 条件が成り立つまで CEF のメッセージを回す。待ちの間は寝ない（寝るとタイマーの粒度が測定に入る）。
pub fn pump_until(
    shared: &Shared,
    what: &str,
    limit: Duration,
    cond: impl Fn(&Shared) -> bool,
) -> Result<(), String> {
    let start = Instant::now();
    while !cond(shared) {
        if let Some(e) = shared.error.borrow_mut().take() {
            return Err(e);
        }
        if start.elapsed() > limit {
            return Err(format!("{what} が {limit:?} 来ない"));
        }
        do_message_loop_work();
        std::hint::spin_loop();
    }
    Ok(())
}

/// 外部 BeginFrame で描くオフスクリーンのブラウザを作る。
pub fn create(
    shared: &Rc<Shared>,
    accelerated: bool,
    rate: u32,
    url: &str,
) -> Result<Browser, String> {
    let window_info = WindowInfo {
        windowless_rendering_enabled: true as _,
        shared_texture_enabled: accelerated as _,
        external_begin_frame_enabled: true as _,
        ..Default::default()
    };
    let browser_settings = BrowserSettings {
        windowless_frame_rate: rate as _,
        ..Default::default()
    };
    browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut build_client(shared)),
        Some(&CefString::from(url)),
        Some(&browser_settings),
        None,
        None,
    )
    .ok_or_else(|| "ブラウザを作れない".to_string())
}

/// JS を実行し、タイトルが `want` になるまで待つ。
pub fn run_js(browser: &Browser, shared: &Shared, code: &str, want: &str) -> Result<(), String> {
    let _span = tracing::info_span!("host_cef.set_time").entered();
    let main = browser.main_frame().ok_or("メインフレームが無い")?;
    main.execute_java_script(Some(&CefString::from(code)), None, 0);
    pump_until(shared, "時刻の反映", WAIT_LIMIT, |s| {
        *s.title.borrow() == want
    })
}

/// BeginFrame を送り、`accept` を満たすフレームが来るまで `resend` ごとに送り直す。
/// `limit` までに来なければ `Ok(false)`。
pub fn begin_frame(
    browser: &Browser,
    shared: &Shared,
    resend: Duration,
    limit: Duration,
    accept: impl Fn(&Shared) -> bool,
) -> Result<bool, String> {
    let _span = tracing::info_span!("host_cef.begin_frame").entered();
    let host = browser.host().ok_or("ブラウザのホストが無い")?;
    let start = Instant::now();
    let mut before = shared.painted.get();
    let mut attempts = 0u64;
    while start.elapsed() < limit {
        host.send_external_begin_frame();
        let sent = Instant::now();
        while shared.painted.get() == before && sent.elapsed() < resend {
            if let Some(e) = shared.error.borrow_mut().take() {
                return Err(e);
            }
            do_message_loop_work();
            std::hint::spin_loop();
        }
        if shared.painted.get() != before {
            if accept(shared) {
                mm_trace::counter!("host_cef.begin_frame_resent", attempts);
                return Ok(true);
            }
            // 大きさを変えた直後の古い大きさのフレームなどは捨てて、次を待つ。
            before = shared.painted.get();
        }
        attempts += 1;
    }
    mm_trace::counter!("host_cef.begin_frame_resent", attempts);
    Ok(false)
}

/// ブラウザを閉じて、閉じる処理が終わるまで少し回す。
pub fn close(browser: &Browser) {
    const DRAIN: Duration = Duration::from_millis(300);
    if let Some(host) = browser.host() {
        host.close_browser(1);
    }
    let end = Instant::now();
    while end.elapsed() < DRAIN {
        do_message_loop_work();
    }
}
