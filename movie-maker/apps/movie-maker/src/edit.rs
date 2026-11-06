//! WebView（Inspector など）から届いた操作の適用。操作は再生スレッドが持つプロジェクトのデータに当て、
//! 触った ID に頼ったフレームのキャッシュと素材だけを `Engine::invalidate_touched` で捨てる。
//!
//! 再生スレッドだけが `Engine` とプロジェクトのデータを持つので、操作は `Shared::edits` に積んで
//! `Command::Edit` で知らせ、結果を返りのチャンネルで受ける。

use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

use mm_ops::Op;
use serde::Serialize;

use crate::playback::Command;
use crate::state::Shared;

/// 再生スレッドが操作を当てるまで待つ上限。描画 1 フレームと適用より十分長い。
const APPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// 当てた結果。WebView に返す。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditOutcome {
    /// 触った ID（レイヤー、素材、コンポジションなど）。
    pub touched: Vec<String>,
}

/// 再生スレッドに渡す、まだ当てていない操作。
pub struct PendingEdit {
    pub ops: Vec<Op>,
    pub reply: Sender<Result<EditOutcome, String>>,
}

/// 操作を 1 つのトランザクションとして再生スレッドに当てさせ、結果を待つ。
pub fn submit(shared: &Shared, ops: Vec<Op>) -> Result<EditOutcome, String> {
    let (reply, rx) = channel();
    shared
        .edits
        .lock()
        .map_err(|_| "操作の待ち行列が壊れた".to_owned())?
        .push(PendingEdit { ops, reply });
    shared.send(Command::Edit);
    rx.recv_timeout(APPLY_TIMEOUT)
        .map_err(|e| format!("再生スレッドが操作を当てない: {e}"))?
}

/// WebView の操作（`ui/sdk` の `apply_op`）。`Op` の JSON をそのまま受ける。
#[tauri::command]
pub async fn apply_op(
    state: tauri::State<'_, Arc<crate::ipc::SyncState>>,
    op: serde_json::Value,
) -> Result<EditOutcome, String> {
    let op: Op = serde_json::from_value(op).map_err(|e| format!("操作の形が違う: {e}"))?;
    let shared = state.shared.clone();
    tauri::async_runtime::spawn_blocking(move || submit(&shared, vec![op]))
        .await
        .map_err(|e| e.to_string())?
}
