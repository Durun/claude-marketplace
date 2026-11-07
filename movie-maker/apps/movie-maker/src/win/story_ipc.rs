//! Story パネルの Tauri のコマンド。名前と引数の形は `ui/sdk/README.md` の「Story のコマンド」。
//! 文字起こしの編集（話題、削除、修正、評価、タグ、並べる）は `apply_op` で送る。

use std::sync::Arc;

use tauri::State;

use crate::win::story::{Overview, StoryHit, StoryState, StoryTranscript, WordRange};

/// このパネルのコマンドを呼び出す。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![
        story_overview,
        story_transcript,
        story_range,
        story_search,
        story_timeline_time,
        story_cut_time,
    ];
    h(invoke)
}

/// 検索結果の件数の既定。`mm search` と同じ。
const DEFAULT_SEARCH_LIMIT: usize = 50;

type Story<'a> = State<'a, Arc<StoryState>>;

async fn blocking<T: Send + 'static>(
    state: Story<'_>,
    f: impl FnOnce(&StoryState) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(&state))
        .await
        .map_err(|e| e.to_string())?
}

/// 素材（文字起こしの有無つき）とシーケンスの一覧。
#[tauri::command]
pub async fn story_overview(state: Story<'_>) -> Result<Overview, String> {
    blocking(state, StoryState::overview).await
}

/// 素材の文字起こし（単語の列、話題、選別）。文字起こしが無い素材は単語が空。
#[tauri::command]
pub async fn story_transcript(state: Story<'_>, asset: String) -> Result<StoryTranscript, String> {
    blocking(state, move |s| s.transcript(&asset)).await
}

/// 単語 `first` から `last` までの時刻（素材の時間軸）。話題の `in` と `out` にそのまま使える。
#[tauri::command]
pub async fn story_range(
    state: Story<'_>,
    asset: String,
    first: String,
    last: String,
) -> Result<WordRange, String> {
    blocking(state, move |s| s.range(&asset, &first, &last)).await
}

/// 文字起こしの全文検索（`mm search` と同じ規則）。
#[tauri::command]
pub async fn story_search(
    state: Story<'_>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<StoryHit>, String> {
    let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    blocking(state, move |s| s.search(&query, limit)).await
}

/// 素材の時刻（秒）に当たる再生位置（秒）。sequence のカットに入っていなければ `None`。
#[tauri::command]
pub async fn story_timeline_time(
    state: Story<'_>,
    asset: String,
    at: f64,
) -> Result<Option<f64>, String> {
    blocking(state, move |s| s.timeline_time(&asset, at)).await
}

/// カットの始まりの再生位置（秒）。開いているコンポジションに映っていなければ `None`。
#[tauri::command]
pub async fn story_cut_time(state: Story<'_>, cut: String) -> Result<Option<f64>, String> {
    blocking(state, move |s| s.cut_time(&cut)).await
}
