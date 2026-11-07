//! Media パネルの Tauri のコマンド。名前と引数の形は `ui/sdk/README.md` の「Media のコマンド」。

use std::sync::Arc;

use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, State};

use crate::win::media::{CacheInfo, ImportEvent, MediaItem, MediaState, WaveformView};

/// このパネルのコマンドを呼び出す。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![
        media_list,
        media_thumbnail,
        media_waveform,
        media_pick_files,
        media_import,
        media_cache_info,
        media_cache_clear,
    ];
    h(invoke)
}

/// 取り込みの進み具合を送る Tauri のイベント名。`ui/sdk` の `INGEST_PROGRESS_EVENT` と同じ。
const INGEST_PROGRESS_EVENT: &str = "mm:ingest-progress";
/// ファイル選択の「動画と音」の拡張子。ffmpeg が読めるものの代表。
const MEDIA_EXTENSIONS: [&str; 14] = [
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "mts", "wav", "mp3", "flac", "m4a", "aac", "ogg",
    "opus",
];

type Media<'a> = State<'a, Arc<MediaState>>;

/// 素材の一覧。
#[tauri::command]
pub async fn media_list(state: Media<'_>) -> Result<Vec<MediaItem>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.list())
        .await
        .map_err(|e| e.to_string())
}

/// サムネイルの JPEG を生のバイト列で返す。
#[tauri::command]
pub async fn media_thumbnail(state: Media<'_>, id: String) -> Result<Response, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.thumbnail(&id))
        .await
        .map_err(|e| e.to_string())?
        .map(Response::new)
}

/// 波形のピーク（`buckets` 個）。
#[tauri::command]
pub async fn media_waveform(
    state: Media<'_>,
    id: String,
    buckets: usize,
) -> Result<WaveformView, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.waveform(&id, buckets))
        .await
        .map_err(|e| e.to_string())?
}

/// ファイルの選択ダイアログを開き、選んだパスを返す。キャンセルは空の配列。選んだパスは取り込んでよいパスとして覚える。
#[tauri::command]
pub async fn media_pick_files(state: Media<'_>) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = rfd::FileDialog::new()
            .add_filter("動画と音", &MEDIA_EXTENSIONS)
            .add_filter("すべてのファイル", &["*"])
            .pick_files()
            .unwrap_or_default();
        let paths = picked
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        state.offer(picked);
        paths
    })
    .await
    .map_err(|e| e.to_string())
}

/// 素材を取り込む。進み具合は `mm:ingest-progress` で送り、全部終わってから返る。
#[tauri::command]
pub async fn media_import(
    app: AppHandle,
    state: Media<'_>,
    paths: Vec<String>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |event: ImportEvent| {
            let _ = app.emit(INGEST_PROGRESS_EVENT, event);
        };
        state.import(&paths, &emit)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `cache/` の大きさ。
#[tauri::command]
pub async fn media_cache_info(state: Media<'_>) -> Result<CacheInfo, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.cache_info())
        .await
        .map_err(|e| e.to_string())
}

/// `cache/` の掃除。空けたバイト数を返す。
#[tauri::command]
pub async fn media_cache_clear(state: Media<'_>, kinds: Vec<String>) -> Result<u64, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.clear_cache(&kinds))
        .await
        .map_err(|e| e.to_string())?
}
