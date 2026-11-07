//! Mixer パネルの Tauri のコマンドとイベント。名前と形は `ui/sdk/README.md` の「Mixer のコマンド」。
//! 設定の変更（`setLayerAudio`、`setMixer`）は `apply_ops` で送る。

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::win::ipc::SyncState;
use crate::win::mixer::{state_of, MixerMeters, MixerState};
use crate::win::state::Shared;

/// このパネルのコマンドを呼び出す。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![mixer_state,];
    h(invoke)
}

/// メーターの値を送る Tauri のイベント名。`ui/sdk` の `MIXER_METERS_EVENT` と同じ。
const MIXER_METERS_EVENT: &str = "mm:mixer-meters";

/// 開いたコンポジションのトラックとミキサーの設定。
#[tauri::command]
pub async fn mixer_state(state: State<'_, Arc<SyncState>>) -> Result<MixerState, String> {
    let doc = state
        .shared
        .doc()
        .ok_or_else(|| "プロジェクトを開いていない".to_owned())?;
    state_of(&doc)
}

/// 再生位置の通知と同じ間隔で、鳴っている位置のメーターの値を送る。止まったら 1 回だけ `playing: false` を送る。
pub fn emit_meters(app: &AppHandle, shared: &Shared, t: f64, was_playing: &mut bool) {
    let playing = shared.playing.load(Ordering::SeqCst);
    if !playing && !*was_playing {
        return;
    }
    *was_playing = playing;
    let reading = shared.meters.lock().expect("meters").at(t);
    let (tracks, master) = reading.unwrap_or_default();
    let meters = MixerMeters {
        t,
        playing,
        tracks,
        master,
    };
    let _ = app.emit(MIXER_METERS_EVENT, meters);
}
