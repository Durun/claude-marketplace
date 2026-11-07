//! Transition パネルの Tauri のコマンド。名前と引数の形は `ui/sdk/README.md` の「Transition のコマンド」。
//! 置く・差し替えるは `pack_apply`（マクロ `transition`）、外すは `apply_op` の `setTransition` で送り、どちらも取り消し 1 回で戻る。

use std::path::PathBuf;
use std::sync::Arc;

use mm_pack::Pack;

use crate::win::ipc::SyncState;
use crate::win::transition::{cuts_of, CutsState};

/// このパネルのコマンドを呼び出す。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![transition_cuts,];
    h(invoke)
}

/// 開いているコンポジションの sequence のカットと、境のトランジション。種類の名前は `dir` のパックのコードと突き合わせる。
#[tauri::command]
pub async fn transition_cuts(
    state: tauri::State<'_, Arc<SyncState>>,
    dir: String,
) -> Result<CutsState, String> {
    let doc = state
        .shared
        .doc()
        .ok_or_else(|| "プロジェクトを開いていない".to_owned())?;
    let pack = Pack::load(&PathBuf::from(dir)).ok();
    cuts_of(&doc.project, &doc.comp, pack.as_ref())
}
