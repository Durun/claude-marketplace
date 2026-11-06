//! キーの表は Rust が 1 つ持つ。サーフェスと WebView のどちらから来ても同じ関数に入れる。

use crate::playback::Command;
use crate::state::Shared;

/// 仮想キーコードをキー名にする（WebView の `KeyboardEvent.code` に寄せた名前）。
pub fn vk_name(vk: u32) -> String {
    match vk {
        0x41..=0x5A => format!("Key{}", char::from_u32(vk).unwrap_or('?')),
        0x30..=0x39 => format!("Digit{}", char::from_u32(vk).unwrap_or('?')),
        0x20 => "Space".to_owned(),
        0x23 => "End".to_owned(),
        0x24 => "Home".to_owned(),
        0x25 => "ArrowLeft".to_owned(),
        0x27 => "ArrowRight".to_owned(),
        _ => format!("Vk{vk:02X}"),
    }
}

/// キーに割り当てた再生のコマンド。
pub fn command_for(key: &str) -> Option<Command> {
    match key {
        "Space" => Some(Command::Toggle),
        "KeyK" => Some(Command::Pause),
        "KeyL" => Some(Command::Play),
        "ArrowLeft" => Some(Command::Step(-1)),
        "ArrowRight" => Some(Command::Step(1)),
        "Home" => Some(Command::Seek(0.0)),
        "End" => Some(Command::Seek(f64::MAX)),
        _ => None,
    }
}

/// キーを 1 つ受けるエントリーポイント。`source` は "surface" か "webview"。
pub fn dispatch(shared: &Shared, source: &str, key: &str) {
    let _span = tracing::info_span!("window.key").entered();
    if let Some(command) = command_for(key) {
        shared.send(command);
    }
    tracing::debug!(source, key, "key");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn 英字と矢印は_code_の名前にする() {
        assert_eq!(vk_name(0x4A), "KeyJ");
        assert_eq!(vk_name(0x20), "Space");
        assert_eq!(vk_name(0x27), "ArrowRight");
    }

    #[test]
    fn サーフェスと_webview_のどちらのキーも同じコマンドになる() {
        let shared = Shared::new();
        let (tx, rx) = channel();
        *shared.commands.lock().unwrap() = Some(tx);
        dispatch(&shared, "surface", "Space");
        dispatch(&shared, "webview", "Space");
        dispatch(&shared, "webview", "KeyZ");
        assert_eq!(rx.try_recv(), Ok(Command::Toggle));
        assert_eq!(rx.try_recv(), Ok(Command::Toggle));
        assert!(rx.try_recv().is_err(), "割り当ての無いキーは何も送らない");
    }
}
