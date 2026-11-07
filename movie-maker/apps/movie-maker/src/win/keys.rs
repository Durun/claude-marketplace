//! キーの表は Rust が 1 つ持つ。サーフェスと WebView のどちらから来ても同じ関数に入れる。

use std::sync::Arc;

use crate::win::edit_actions::{self, EditAction};
use crate::win::playback::Command;
use crate::win::state::Shared;

/// 仮想キーコードをキー名にする（WebView の `KeyboardEvent.code` に寄せた名前）。
pub fn vk_name(vk: u32) -> String {
    match vk {
        0x41..=0x5A => format!("Key{}", char::from_u32(vk).unwrap_or('?')),
        0x30..=0x39 => format!("Digit{}", char::from_u32(vk).unwrap_or('?')),
        0x20 => "Space".to_owned(),
        0x23 => "End".to_owned(),
        0x24 => "Home".to_owned(),
        0x08 => "Backspace".to_owned(),
        0x2E => "Delete".to_owned(),
        0xBC => "Comma".to_owned(),
        0xBE => "Period".to_owned(),
        0x25 => "ArrowLeft".to_owned(),
        0x27 => "ArrowRight".to_owned(),
        _ => format!("Vk{vk:02X}"),
    }
}

/// 修飾キーをキー名の前に付ける。Ctrl と Alt は常に、Shift は Ctrl と一緒のときだけ付ける
/// （Shift を押しながらの Space や矢印は、これまでどおりそのキーとして扱う）。
pub fn with_modifiers(name: &str, ctrl: bool, shift: bool, alt: bool) -> String {
    let mut out = String::new();
    if ctrl {
        out.push_str("Ctrl+");
        if shift {
            out.push_str("Shift+");
        }
    }
    if alt {
        out.push_str("Alt+");
    }
    out.push_str(name);
    out
}

/// キーに割り当てた編集。Source のイン点・アウト点、insert / overwrite、split、lift / extract、取り消しとやり直し。
pub fn edit_for(key: &str) -> Option<EditAction> {
    match key {
        "KeyI" => Some(EditAction::MarkIn),
        "KeyO" => Some(EditAction::MarkOut),
        "Comma" => Some(EditAction::Insert),
        "Period" => Some(EditAction::Overwrite),
        "KeyB" => Some(EditAction::Split),
        "Delete" => Some(EditAction::Lift),
        "Backspace" => Some(EditAction::Extract),
        "Ctrl+KeyZ" => Some(EditAction::Undo),
        "Ctrl+KeyY" | "Ctrl+Shift+KeyZ" => Some(EditAction::Redo),
        _ => None,
    }
}

/// キーに割り当てた再生のコマンド。
pub fn command_for(key: &str) -> Option<Command> {
    match key {
        "Space" => Some(Command::Toggle),
        "KeyJ" => Some(Command::Reverse),
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
/// 再生のキーは、Source の領域をクリックしていれば Source の、そうでなければ Program の再生に向く。
pub fn dispatch(shared: &Arc<Shared>, source: &str, key: &str) {
    let _span = tracing::info_span!("window.key").entered();
    if let Some(action) = edit_for(key) {
        edit_actions::run(shared, action);
    } else if let Some(command) = command_for(key) {
        if shared.source.is_active() && shared.source.handle().is_some() {
            shared.source.send(command);
        } else {
            shared.send(command);
        }
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
        let shared = Arc::new(Shared::new());
        let (tx, rx) = channel();
        *shared.commands.lock().unwrap() = Some(tx);
        dispatch(&shared, "surface", "Space");
        dispatch(&shared, "webview", "Space");
        dispatch(&shared, "webview", "KeyZ");
        assert_eq!(rx.try_recv(), Ok(Command::Toggle));
        assert_eq!(rx.try_recv(), Ok(Command::Toggle));
        assert!(rx.try_recv().is_err(), "割り当ての無いキーは何も送らない");
    }

    #[test]
    fn 修飾キーはキー名の前に付く_shift_は_ctrl_と一緒のときだけ() {
        assert_eq!(with_modifiers("KeyZ", true, false, false), "Ctrl+KeyZ");
        assert_eq!(with_modifiers("KeyZ", true, true, false), "Ctrl+Shift+KeyZ");
        assert_eq!(with_modifiers("Space", false, true, false), "Space");
        assert_eq!(with_modifiers("KeyX", false, false, true), "Alt+KeyX");
    }

    #[test]
    fn 編集のキーの割り当て() {
        assert_eq!(edit_for("KeyI"), Some(EditAction::MarkIn));
        assert_eq!(edit_for("Comma"), Some(EditAction::Insert));
        assert_eq!(edit_for("Period"), Some(EditAction::Overwrite));
        assert_eq!(edit_for("KeyB"), Some(EditAction::Split));
        assert_eq!(edit_for("Delete"), Some(EditAction::Lift));
        assert_eq!(edit_for("Backspace"), Some(EditAction::Extract));
        assert_eq!(edit_for("Ctrl+KeyZ"), Some(EditAction::Undo));
        assert_eq!(edit_for("Ctrl+Shift+KeyZ"), Some(EditAction::Redo));
        assert_eq!(edit_for("KeyZ"), None);
        assert_eq!(command_for("KeyJ"), Some(Command::Reverse));
    }

    #[test]
    fn source_を開いて向けていれば再生のキーは_source_に行く() {
        let shared = Arc::new(Shared::new());
        let (tx, rx) = channel();
        *shared.commands.lock().unwrap() = Some(tx);
        shared.source.set_active(true);
        dispatch(&shared, "surface", "KeyL");
        assert_eq!(rx.try_recv(), Ok(Command::Play), "素材が無ければ Program");
    }
}
