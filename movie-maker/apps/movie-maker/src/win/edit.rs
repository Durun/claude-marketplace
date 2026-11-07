//! WebView（Inspector など）とキーとドラッグから届いた操作の適用。
//!
//! `project.mmdb` を書き手として開いていれば `Core` に保存し、保存した後のデータを `Shared::doc` に置き直してから返る
//! （返った直後に読み直す Story や Media が古いデータを見ないように）。描き直しは変更の通知を受けた再生スレッドが行う。
//! JSON のプロジェクトでは、再生スレッドだけが `Engine` とデータを持つので、操作は `Shared::edits` に積んで
//! `Command::Edit` で知らせ、結果を返りのチャンネルで受ける。

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

use mm_ops::Op;
use serde::Serialize;

use crate::win::history::HistoryMirror;
use crate::win::playback::Command;
use crate::win::state::Shared;

/// 再生スレッドが操作を適用するまで待つ上限。描画 1 フレームと適用より十分長い。
const APPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// WebView から届いた操作のトランザクションの名前。
const WEBVIEW_TXN: &str = "WebView の操作";

/// 適用した結果。WebView に返す。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditOutcome {
    /// 触った ID（レイヤー、素材、コンポジションなど）。
    pub touched: Vec<String>,
}

/// 操作の持ち主の状態。待っている間は、再生スレッドが適用すると決めるか、送った側が取り下げるかの早い者勝ち。
const WAITING: u8 = 0;
const CLAIMED: u8 = 1;
const WITHDRAWN: u8 = 2;

/// 再生スレッドに渡す、まだ適用していない操作。
pub struct PendingEdit {
    pub ops: Vec<Op>,
    pub reply: Sender<Result<EditOutcome, String>>,
    state: Arc<AtomicU8>,
}

impl PendingEdit {
    /// 適用する権利を取る。送った側が時間切れで取り下げた操作は偽を返すので、適用せずに捨てる。
    pub fn claim(&self) -> bool {
        self.state
            .compare_exchange(WAITING, CLAIMED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

/// 編集のキューに積む仕事。計画から適用までを 1 つの仕事にする。
pub type EditJob = Box<dyn FnOnce() + Send>;

/// 計画と適用を 1 本のスレッドで順に回す。キーを続けて押しても、前の編集を適用した後のデータで次を計画する。
pub fn queue(shared: &Shared, job: impl FnOnce() + Send + 'static) {
    let mut lane = shared.edit_lane.lock().expect("edit lane");
    if lane.is_none() {
        let (tx, rx) = channel::<EditJob>();
        let spawned = std::thread::Builder::new()
            .name("edit-lane".into())
            .spawn(move || rx.into_iter().for_each(|job| job()));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "編集のスレッドを作れない");
            return;
        }
        *lane = Some(tx);
    }
    if let Some(tx) = lane.as_ref() {
        let _ = tx.send(Box::new(job));
    }
}

/// 操作を 1 つのトランザクションとして今のバージョンに適用し、結果を待つ。
pub fn submit(shared: &Shared, ops: Vec<Op>) -> Result<EditOutcome, String> {
    submit_based(shared, ops, None)
}

/// 操作を、計画に使ったバージョン `base` を元にしたトランザクションとして適用する。
/// その後に別の発行元（CLI など）が同じ所を触っていたら、衝突として拒否する。JSON のプロジェクトでは `base` を見ない。
pub fn submit_based(
    shared: &Shared,
    ops: Vec<Op>,
    base: Option<i64>,
) -> Result<EditOutcome, String> {
    match shared.core.get() {
        Some(core) => apply_saved(shared, core, ops, base),
        None => submit_within(shared, ops, APPLY_TIMEOUT),
    }
}

/// 待ちの上限を引数で決められる `submit`。時間切れで返した `Err` は「適用していない」を意味する。
/// 再生スレッドがもう適用し始めていたら、結果が出るまで待つ（失敗と答えて再送で二重に適用されるのを防ぐ）。
pub fn submit_within(
    shared: &Shared,
    ops: Vec<Op>,
    limit: Duration,
) -> Result<EditOutcome, String> {
    let (reply, rx) = channel();
    let state = Arc::new(AtomicU8::new(WAITING));
    shared
        .edits
        .lock()
        .map_err(|_| "操作の待ち行列が壊れた".to_owned())?
        .push(PendingEdit {
            ops,
            reply,
            state: state.clone(),
        });
    shared.send(Command::Edit);
    match rx.recv_timeout(limit) {
        Ok(result) => result,
        Err(_) => {
            let withdrawn =
                state.compare_exchange(WAITING, WITHDRAWN, Ordering::SeqCst, Ordering::SeqCst);
            if withdrawn.is_ok() {
                return Err(
                    "再生スレッドが時間内に操作を適用しなかったので取り下げた（適用されていない）"
                        .into(),
                );
            }
            rx.recv()
                .map_err(|_| "再生スレッドが結果を返す前に終わった".to_owned())?
        }
    }
}

/// WebView の操作（`ui/sdk` の `apply_op`）。`Op` の JSON をそのまま受ける。
#[tauri::command]
pub async fn apply_op(
    state: tauri::State<'_, Arc<crate::win::ipc::SyncState>>,
    op: serde_json::Value,
) -> Result<EditOutcome, String> {
    let op: Op = serde_json::from_value(op).map_err(|e| format!("操作の形が違う: {e}"))?;
    let shared = state.shared.clone();
    tauri::async_runtime::spawn_blocking(move || submit(&shared, vec![op]))
        .await
        .map_err(|e| e.to_string())?
}

/// WebView の操作をまとめて 1 つのトランザクションとして適用する（取り消し 1 回で戻る）。
/// レンダーターゲットを直すときの「外して足す」組のように、途中の状態を残したくない操作に使う。
#[tauri::command]
pub async fn apply_ops(
    state: tauri::State<'_, Arc<crate::win::ipc::SyncState>>,
    ops: Vec<serde_json::Value>,
) -> Result<EditOutcome, String> {
    let ops = ops
        .into_iter()
        .map(|op| serde_json::from_value::<Op>(op).map_err(|e| format!("操作の形が違う: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let shared = state.shared.clone();
    tauri::async_runtime::spawn_blocking(move || submit(&shared, ops))
        .await
        .map_err(|e| e.to_string())?
}

/// 書き手の `Store` に適用して `project.mmdb` に書き、書いた後のデータを置き直す。
fn apply_saved(
    shared: &Shared,
    core: &mm_serve::Core,
    ops: Vec<Op>,
    base: Option<i64>,
) -> Result<EditOutcome, String> {
    let mut txn = mm_ops::Transaction::new(WEBVIEW_TXN, ops);
    if let Some(version) = base {
        txn = txn.based_on(version.max(0) as u64);
    }
    let change = core
        .apply(txn, &mm_serve::Caller::webview(), None)
        .map_err(|e| e.to_string())?;
    shared.history.record(&change);
    refresh_saved(shared, core)?;
    Ok(EditOutcome {
        touched: change.touched,
    })
}

/// 保存した最新のプロジェクトのデータを `Shared::doc` に置く。置いてあるものより新しいときだけ置き直す。
pub fn refresh_saved(shared: &Shared, core: &mm_serve::Core) -> Result<(), String> {
    let Some(comp) = shared.doc().map(|d| d.comp) else {
        return Ok(());
    };
    let (version, project) = core.render_project().map_err(|e| e.to_string())?;
    shared.publish_saved(version, Arc::new(project), comp);
    Ok(())
}

/// `project.mmdb` の直前のトランザクションを取り消す（`undo` が偽ならやり直す）。発行元を問わず戻し、
/// 何を戻したか（人でない発行元ならその名前も）を編集の文言に出す。
pub fn step_saved(shared: &Shared, core: &mm_serve::Core, undo: bool) {
    let before = shared.history.next(undo);
    let caller = mm_serve::Caller::webview();
    let done = if undo {
        core.undo(&caller)
    } else {
        core.redo(&caller)
    };
    let done = done.map_err(|e| e.to_string());
    if let Ok(Some(change)) = &done {
        shared.history.record(change);
        if let Err(e) = refresh_saved(shared, core) {
            tracing::warn!("保存したプロジェクトのデータを読めない: {e}");
        }
    }
    let failed = !matches!(done, Ok(Some(_)));
    let message = HistoryMirror::message(undo, done, before);
    if failed {
        shared.edit.set_message(message);
    } else {
        shared.edit.notify(message);
    }
}

/// 保存した変更（HTTP と IPC のどちらから書かれても）を受けたときの仕事。データを置き直してから、
/// 再生スレッドに描き直させる。画面への通知は、これが返った後に送る（読み直した側が新しいデータを見る）。
pub fn relay_change(shared: &Shared, change: &mm_serve::Change) {
    shared.history.record(change);
    if let Some(core) = shared.core.get() {
        if let Err(e) = refresh_saved(shared, core) {
            tracing::warn!("保存したプロジェクトのデータを読めない: {e}");
        }
    }
    shared
        .reload_touched
        .lock()
        .expect("reload")
        .extend(change.touched.iter().cloned());
    shared.send(Command::Reload);
}

/// `Core` の変更を受けるスレッドを始める。変更ごとに [`relay_change`] でデータを置き直してから `notify` を呼ぶ。
/// JSON のプロジェクト（`Core` が無い）では何もしない。
pub fn watch_changes(
    shared: &Arc<Shared>,
    mut notify: impl FnMut(&mm_serve::Change) + Send + 'static,
) -> std::io::Result<()> {
    let Some(core) = shared.core.get() else {
        return Ok(());
    };
    let shared = shared.clone();
    core.on_change(move |change| {
        relay_change(&shared, &change);
        notify(&change);
    })
    .map(|_| ())
}

#[cfg(test)]
mod tests;
