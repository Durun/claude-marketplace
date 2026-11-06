//! WebView（Inspector など）から届いた操作の適用。操作は再生スレッドが持つプロジェクトのデータに当て、
//! 触った ID に頼ったフレームのキャッシュと素材だけを `Engine::invalidate_touched` で捨てる。
//!
//! 再生スレッドだけが `Engine` とプロジェクトのデータを持つので、操作は `Shared::edits` に積んで
//! `Command::Edit` で知らせ、結果を返りのチャンネルで受ける。

use std::sync::atomic::{AtomicU8, Ordering};
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

/// 操作の持ち主の状態。待っている間は、再生スレッドが当てると決めるか、送った側が取り下げるかの早い者勝ち。
const WAITING: u8 = 0;
const CLAIMED: u8 = 1;
const WITHDRAWN: u8 = 2;

/// 再生スレッドに渡す、まだ当てていない操作。
pub struct PendingEdit {
    pub ops: Vec<Op>,
    pub reply: Sender<Result<EditOutcome, String>>,
    state: Arc<AtomicU8>,
}

impl PendingEdit {
    /// 当てる権利を取る。送った側が時間切れで取り下げた操作は偽を返すので、当てずに捨てる。
    pub fn claim(&self) -> bool {
        self.state
            .compare_exchange(WAITING, CLAIMED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

/// 操作を 1 つのトランザクションとして再生スレッドに当てさせ、結果を待つ。
pub fn submit(shared: &Shared, ops: Vec<Op>) -> Result<EditOutcome, String> {
    submit_within(shared, ops, APPLY_TIMEOUT)
}

/// `submit` の待ちの上限を決められる版。時間切れで返した `Err` は「当てていない」を意味する。
/// 再生スレッドがもう当て始めていたら、結果が出るまで待つ（失敗と答えて再送で二重に当たるのを防ぐ）。
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
                    "再生スレッドが時間内に操作を当てなかったので取り下げた（当たっていない）"
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
    state: tauri::State<'_, Arc<crate::ipc::SyncState>>,
    op: serde_json::Value,
) -> Result<EditOutcome, String> {
    let op: Op = serde_json::from_value(op).map_err(|e| format!("操作の形が違う: {e}"))?;
    let shared = state.shared.clone();
    tauri::async_runtime::spawn_blocking(move || submit(&shared, vec![op]))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op() -> Op {
        mm_ops::SetParam {
            target: mm_ops::ParamRef::layer(&mm_types::Id::new("l"), "opacity"),
            value: mm_types::ParamValue::Number(0.5),
            keyframes: None,
        }
        .into()
    }

    #[test]
    fn 時間切れで返した操作は後から当たらない() {
        let shared = Shared::new();
        let r = submit_within(&shared, vec![op()], Duration::from_millis(20));
        assert!(r.is_err());
        let late = shared.edits.lock().unwrap().pop().expect("積まれたまま");
        assert!(!late.claim(), "取り下げた操作は当てる権利を取れない");
    }

    #[test]
    fn 当て始めた操作は時間を超えても結果を待つ() {
        let shared = Arc::new(Shared::new());
        let s = shared.clone();
        let player = std::thread::spawn(move || {
            let edit = loop {
                if let Some(e) = s.edits.lock().unwrap().pop() {
                    break e;
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            assert!(edit.claim());
            std::thread::sleep(Duration::from_millis(80));
            let _ = edit.reply.send(Ok(EditOutcome {
                touched: vec!["l".into()],
            }));
        });
        let r = submit_within(&shared, vec![op()], Duration::from_millis(30));
        player.join().unwrap();
        assert_eq!(r.expect("当たった結果を返す").touched, ["l"]);
    }
}
