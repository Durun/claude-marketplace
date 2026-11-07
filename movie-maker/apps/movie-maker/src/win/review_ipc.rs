//! Review パネルの Tauri のコマンド。名前と引数の形は `ui/sdk/README.md` の「Review のコマンド」。
//! 決定は人の操作経路（WebView）として `Core` を通るので、承認したトランザクションは Ctrl+Z で戻る。

use std::sync::Arc;

use mm_serve::{Approval, Caller, Core, ProposalView};
use tauri::State;

use crate::win::review::{self, Preview};
use crate::win::state::Shared;

/// このパネルのコマンドを呼び出す。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![
        review_list,
        review_approve,
        review_reject,
        review_reopen,
        review_undo,
        review_preview,
    ];
    h(invoke)
}

/// Review パネルが使うもの。
pub struct ReviewState {
    shared: Arc<Shared>,
}

impl ReviewState {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    fn core(&self) -> Result<Arc<Core>, String> {
        self.shared.core.get().cloned().ok_or_else(review::no_store)
    }
}

type Review<'a> = State<'a, Arc<ReviewState>>;

async fn blocking<T: Send + 'static>(
    state: Review<'_>,
    f: impl FnOnce(&ReviewState) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(&state))
        .await
        .map_err(|e| e.to_string())?
}

/// 決めた提案を Program に適用していれば、元のフレームに戻す（承認した後はデータ自体が提案を含む）。
fn end_preview(state: &ReviewState, id: i64) -> Result<(), String> {
    match state.shared.review_preview.get().map(|p| p.id) {
        Some(current) if current == id => review::set_preview(&state.shared, None),
        _ => Ok(()),
    }
}

/// 全部の提案（新しい順）。
#[tauri::command]
pub async fn review_list(state: Review<'_>) -> Result<Vec<ProposalView>, String> {
    blocking(state, |s| s.core()?.proposals().map_err(|e| e.to_string())).await
}

/// 保留中の提案を承認して適用する。`ops` は承認する操作の番号（`None` は全部）。
#[tauri::command]
pub async fn review_approve(
    state: Review<'_>,
    id: i64,
    ops: Option<Vec<usize>>,
) -> Result<Approval, String> {
    blocking(state, move |s| {
        let approval = s
            .core()?
            .approve(id, ops.as_deref(), &Caller::webview())
            .map_err(|e| e.to_string())?;
        end_preview(s, id)?;
        Ok(approval)
    })
    .await
}

/// 保留中の提案を却下する。
#[tauri::command]
pub async fn review_reject(state: Review<'_>, id: i64) -> Result<ProposalView, String> {
    blocking(state, move |s| {
        let proposal = s
            .core()?
            .reject(id, &Caller::webview())
            .map_err(|e| e.to_string())?;
        end_preview(s, id)?;
        Ok(proposal)
    })
    .await
}

/// 却下した提案を保留に戻す。
#[tauri::command]
pub async fn review_reopen(state: Review<'_>, id: i64) -> Result<ProposalView, String> {
    blocking(state, move |s| {
        s.core()?
            .reopen(id, &Caller::webview())
            .map_err(|e| e.to_string())
    })
    .await
}

/// 承認で適用したトランザクションが取り消しのスタックの一番上なら取り消す（Ctrl+Z と同じ経路）。
#[tauri::command]
pub async fn review_undo(state: Review<'_>, id: i64) -> Result<ProposalView, String> {
    blocking(state, move |s| {
        let core = s.core()?;
        review::undo_approved(&s.shared, &core, id)
    })
    .await
}

/// Program に提案を適用したフレームを出す。`id` が `None` なら元のフレームに戻す。
#[tauri::command]
pub async fn review_preview(
    state: Review<'_>,
    id: Option<i64>,
    ops: Option<Vec<usize>>,
) -> Result<(), String> {
    blocking(state, move |s| {
        let preview = id.map(|id| Preview { id, ops });
        review::set_preview(&s.shared, preview)
    })
    .await
}
