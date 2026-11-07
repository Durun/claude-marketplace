//! Source のビューアーと編集のボタンを WebView につなぐ Tauri のコマンドと、状態のイベント（`mm:source`）。

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::win::edit_actions::{self, EditAction};
use crate::win::ipc::SyncState;
use crate::win::playback::Command;
use crate::win::source::{self, SourceState};
use crate::win::state::Shared;

/// このパネルのコマンドを呼び出す。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![
        source_open,
        source_close,
        source_state,
        source_transport,
        edit_action,
    ];
    h(invoke)
}

/// WebView へ Source の状態を送る Tauri のイベント名。`web/transport.js` が受ける。
const SOURCE_EVENT: &str = "mm:source";

/// Source の状態の変化を見分ける値。位置は 1 フレームより細かく見分ける必要はないが、比べやすいよう bit のまま持つ。
type Fingerprint = (u64, u64, bool, u64, u64, u64);

fn fingerprint(shared: &Shared) -> Fingerprint {
    let (t, playing) = shared.source.handle().map_or((0.0, false), |h| {
        (h.shared.playhead(), h.shared.playing.load(Ordering::SeqCst))
    });
    (
        shared.source.serial.load(Ordering::SeqCst),
        t.to_bits(),
        playing,
        shared.doc_serial.load(Ordering::SeqCst),
        shared.edit.cut_serial.load(Ordering::SeqCst),
        shared.edit.message_serial.load(Ordering::SeqCst),
    )
}

/// 状態が変わっていたら WebView に送る。同期スレッドの周期から呼ぶ。
pub fn emit_if_changed(app: &AppHandle, shared: &Shared, sent: &mut Option<Fingerprint>) {
    let now = fingerprint(shared);
    if *sent == Some(now) {
        return;
    }
    *sent = Some(now);
    let _ = app.emit(SOURCE_EVENT, source::state_of(shared));
}

/// 素材を Source で開く。Media で選んだ素材の ID を渡す。
#[tauri::command]
pub async fn source_open(
    state: State<'_, Arc<SyncState>>,
    asset: String,
) -> Result<SourceState, String> {
    let shared = state.shared.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let doc = shared.doc().ok_or("プロジェクトを開いていない")?;
        let comp = doc
            .project
            .composition(&doc.comp)
            .map_err(|e| e.to_string())?;
        shared.source.open(&doc.project, comp, &asset)?;
        Ok(source::state_of(&shared))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Source を閉じる。
#[tauri::command]
pub async fn source_close(state: State<'_, Arc<SyncState>>) -> Result<(), String> {
    let shared = state.shared.clone();
    tauri::async_runtime::spawn_blocking(move || shared.source.close())
        .await
        .map_err(|e| e.to_string())
}

/// いまの Source の状態。
#[tauri::command]
pub async fn source_state(state: State<'_, Arc<SyncState>>) -> Result<SourceState, String> {
    Ok(source::state_of(&state.shared))
}

/// Source の再生の操作。`action` は `transport` と同じ（play、pause、toggle、seek、step、home、end）に reverse が加わる。
#[tauri::command]
pub async fn source_transport(
    state: State<'_, Arc<SyncState>>,
    action: String,
    value: Option<f64>,
) -> Result<(), String> {
    let command = match (action.as_str(), value) {
        ("play", _) => Command::Play,
        ("pause", _) => Command::Pause,
        ("toggle", _) => Command::Toggle,
        ("reverse", _) => Command::Reverse,
        ("seek", Some(t)) => Command::Seek(t),
        ("step", Some(n)) => Command::Step(n as i64),
        ("home", _) => Command::Seek(0.0),
        ("end", _) => Command::Seek(f64::MAX),
        _ => return Err(format!("知らない再生の操作: {action}")),
    };
    state.shared.source.set_active(true);
    state.shared.source.send(command);
    Ok(())
}

/// 名前のついた編集（mark_in、mark_out、insert、overwrite、split、lift、extract、undo、redo）。
#[tauri::command]
pub async fn edit_action(state: State<'_, Arc<SyncState>>, action: String) -> Result<(), String> {
    let action =
        EditAction::parse(&action).ok_or_else(|| format!("知らない編集の名前: {action}"))?;
    edit_actions::run(&state.shared, action);
    Ok(())
}
