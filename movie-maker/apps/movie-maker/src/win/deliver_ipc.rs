//! Deliver パネルの Tauri のコマンド。名前と引数の形は `ui/sdk/README.md` の「Deliver のコマンド」。
//! レンダーターゲットの編集（足す、直す、消す）は `apply_ops` で送る。

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::win::deliver::{
    export_config, output_extensions, DeliverState, Emit, ExportStatus, Overview, StartRequest,
};

/// このパネルのコマンドを呼び出す。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![
        export_overview,
        export_status,
        export_start,
        export_cancel,
        export_reveal,
        export_pick_output,
    ];
    h(invoke)
}

/// 書き出しのキューの状態を送る Tauri のイベント名。`ui/sdk` の `EXPORT_PROGRESS_EVENT` と同じ。
const EXPORT_PROGRESS_EVENT: &str = "mm:export-progress";

type Deliver<'a> = State<'a, Arc<DeliverState>>;

/// レンダーターゲット、コンポジション、sequence の一覧。
#[tauri::command]
pub async fn export_overview(state: Deliver<'_>) -> Result<Overview, String> {
    state.overview()
}

/// 書き出しのキューのいまの状態。パネルを開き直したときに読む。
#[tauri::command]
pub async fn export_status(state: Deliver<'_>) -> Result<ExportStatus, String> {
    Ok(state.status())
}

/// キューを作って書き出しを始め、すぐ返る。進み具合は `mm:export-progress` で送る。
#[tauri::command]
pub async fn export_start(
    app: AppHandle,
    state: Deliver<'_>,
    request: StartRequest,
) -> Result<(), String> {
    let emit: Emit = Arc::new(move |status| {
        let _ = app.emit(EXPORT_PROGRESS_EVENT, status);
    });
    state.start(request, export_config(), emit)
}

/// 書き出しを取り消す。
#[tauri::command]
pub async fn export_cancel(state: Deliver<'_>) -> Result<(), String> {
    state.cancel();
    Ok(())
}

/// 書き出したファイルをエクスプローラーで選んで開く。この書き出しで作ったファイル以外は拒否する。
#[tauri::command]
pub async fn export_reveal(state: Deliver<'_>, path: String) -> Result<(), String> {
    let path = state.revealable(&path)?;
    // explorer は `/select,"パス"` を 1 つの引数として読むので、引用符つきで渡す。
    Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 出力先を選ぶ保存ダイアログ。`codec` に合う拡張子で絞る。キャンセルは `None`。
#[tauri::command]
pub async fn export_pick_output(
    current: Option<String>,
    codec: Option<String>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = rfd::FileDialog::new();
        if let Some(codec) = codec.as_deref() {
            dialog = dialog.add_filter("Video", output_extensions(codec));
        }
        if let Some(path) = current.as_deref().map(std::path::Path::new) {
            if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = path.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
        }
        dialog.save_file().map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())
}
