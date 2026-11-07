//! ページの遷移と資源の要求を [`mm_host_cef::policy`] で拒否するハンドラ。ポップアップも開かせない。
//!
//! 通すページの URL は、`Prepare` の後の最初のメインフレームの遷移（ホストが開いたページ）の URL を Chromium が
//! 正規化した形のまま覚える。ファイルのパスから組んだ URL と比べると、パーセントエンコードの違いで食い違うため。

use std::sync::{Arc, Mutex, MutexGuard};

use cef::*;
use mm_host_cef::policy;

/// 拒否した URL を標準エラーに書くときの長さの上限（`data:` の URL は長い）。
const LOG_URL_CHARS: usize = 120;

/// 通すページの URL。空なら次のメインフレームの遷移を通して覚える。
/// 資源の要求は CEF の IO スレッドで調べるので、UI スレッドの `Shared` とは別に `Mutex` で持つ。
pub type AllowedPage = Arc<Mutex<String>>;

fn lock(page: &AllowedPage) -> MutexGuard<'_, String> {
    page.lock().unwrap_or_else(|e| e.into_inner())
}

/// 次に開くページを通せるよう、覚えた URL を忘れる。
pub fn reset(page: &AllowedPage) {
    lock(page).clear();
}

fn url_of(request: Option<&mut Request>) -> String {
    request
        .map(|r| CefString::from(&r.url()).to_string())
        .unwrap_or_default()
}

fn log_blocked(what: &str, url: &str) {
    let short: String = url.chars().take(LOG_URL_CHARS).collect();
    eprintln!("mm-host-cef: {what}を拒否した: {short}");
}

wrap_resource_request_handler! {
    pub struct GuardResourceHandler {
        page: AllowedPage,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let url = url_of(request);
            if policy::allows_resource(&url, &lock(&self.page)) {
                return ReturnValue::CONTINUE;
            }
            log_blocked("資源の要求", &url);
            ReturnValue::CANCEL
        }
    }
}

wrap_request_handler! {
    pub struct GuardRequestHandler {
        page: AllowedPage,
        resource: ResourceRequestHandler,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let url = url_of(request);
            let main = frame.is_some_and(|f| f.is_main() == 1);
            let mut page = lock(&self.page);
            if main && page.is_empty() {
                page.clone_from(&url);
                return false as _;
            }
            if policy::allows_navigation(&url, &page) {
                return false as _;
            }
            log_blocked("遷移", &url);
            true as _
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            Some(self.resource.clone())
        }
    }
}

wrap_life_span_handler! {
    pub struct GuardLifeSpanHandler {}

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            log_blocked("ポップアップ", &target_url.map(ToString::to_string).unwrap_or_default());
            true as _
        }
    }
}

/// Chromium のネットワークを、届かないプロキシ（閉じたポート）へ向ける。ループバックも外さない。
/// `RequestHandler` は遷移と要求を拒否するが、遷移を始めた時点の先読みの接続（preconnect）と名前解決は
/// その前に始まり、外のサーバーへ TCP の接続だけが届くため。プロキシに向ければ行き先へは何も出ない。
pub fn isolate_network(command_line: &mut CommandLine) {
    /// discard（9 番）。ふつうは閉じていて、つなごうとしても拒否される。
    const DEAD_PROXY: &str = "http://127.0.0.1:9";
    command_line.append_switch_with_value(Some(&"proxy-server".into()), Some(&DEAD_PROXY.into()));
    command_line.append_switch_with_value(
        Some(&"proxy-bypass-list".into()),
        Some(&"<-loopback>".into()),
    );
}

/// 遷移と要求を拒否するハンドラ。
pub fn request_handler(page: &AllowedPage) -> RequestHandler {
    let resource = GuardResourceHandler::new(page.clone());
    GuardRequestHandler::new(page.clone(), resource)
}

/// ポップアップを開かせないハンドラ。
pub fn life_span_handler() -> LifeSpanHandler {
    GuardLifeSpanHandler::new()
}
