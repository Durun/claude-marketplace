//! キーの表は Rust が 1 つ持つ。面と WebView のどちらから来ても同じ入口に入れる。

use std::sync::atomic::Ordering;

use crate::shared::Shared;

/// 仮想キーコードをキー名にする（WebView の `KeyboardEvent.code` に寄せた名前）。
pub fn vk_name(vk: u32) -> String {
    match vk {
        0x41..=0x5A => format!("Key{}", char::from_u32(vk).unwrap_or('?')),
        0x30..=0x39 => format!("Digit{}", char::from_u32(vk).unwrap_or('?')),
        0x20 => "Space".to_owned(),
        _ => format!("Vk{vk:02X}"),
    }
}

/// キーを 1 つ受ける入口。`source` は "surface" か "webview"。
pub fn dispatch(shared: &Shared, source: &str, key: &str) {
    let _span = tracing::info_span!("window.key").entered();
    let stamp = shared.input_stamp_us.swap(0, Ordering::SeqCst);
    if stamp != 0 {
        let ms = shared.now_us().saturating_sub(stamp) as f64 / 1000.0;
        shared.record(
            if source == "surface" {
                "key_to_rust_surface_ms"
            } else {
                "key_to_rust_webview_ms"
            },
            ms,
        );
    }
    if source == "surface" {
        shared.keys_surface.fetch_add(1, Ordering::SeqCst);
        shared.flash_us.store(shared.now_us(), Ordering::SeqCst);
    } else {
        shared.keys_webview.fetch_add(1, Ordering::SeqCst);
    }
    tracing::debug!(source, key, "key");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 英字は_code_の名前にする() {
        assert_eq!(vk_name(0x4A), "KeyJ");
        assert_eq!(vk_name(0x20), "Space");
    }

    #[test]
    fn どちらの入口でも同じ表に数える() {
        let s = Shared::new();
        dispatch(&s, "surface", "KeyJ");
        dispatch(&s, "webview", "KeyJ");
        assert_eq!(s.keys_surface.load(Ordering::SeqCst), 1);
        assert_eq!(s.keys_webview.load(Ordering::SeqCst), 1);
    }
}
