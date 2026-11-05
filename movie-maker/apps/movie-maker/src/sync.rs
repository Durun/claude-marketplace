//! 再生位置を Rust から WebView へ送り、画面に出るまでの遅れを測る。
//!
//! 送った時刻の壁時計と、WebView が受けた時刻・rAF の時刻を突き合わせて片道を出し、
//! `window.sync` のスパンは送ってから ack が戻るまでの往復を測る。

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tracing::Span;

use crate::keys;
use crate::shared::{wall_us, Shared, UiSize, SYNC_CHANNEL, SYNC_EMIT, SYNC_OFF};

/// 再生位置の通知。
#[derive(Clone, Serialize)]
pub struct Tick {
    pub seq: u64,
    /// 再生位置（秒）。
    pub t: f64,
    pub mode: u8,
}

struct Pending {
    span: Span,
    sent_wall_us: u64,
}

/// 同期の状態。Tauri の `manage` に載せる。
pub struct SyncState {
    shared: Arc<Shared>,
    channel: Mutex<Option<Channel<Tick>>>,
    pending: Mutex<HashMap<u64, Pending>>,
}

impl SyncState {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            channel: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
        }
    }
}

/// 60 Hz で再生位置を送るスレッドを始める。
pub fn spawn(app: AppHandle, state: Arc<SyncState>) {
    std::thread::Builder::new()
        .name("sync".into())
        .spawn(move || {
            let mut seq = 0u64;
            let mut deadline = Instant::now();
            loop {
                deadline += Duration::from_nanos(16_666_667);
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                }
                let mode = state.shared.sync_mode.load(Ordering::SeqCst);
                if mode == SYNC_OFF {
                    continue;
                }
                seq += 1;
                let tick = Tick {
                    seq,
                    t: state.shared.t0.elapsed().as_secs_f64(),
                    mode,
                };
                // span は ack が戻って pending から外れたときに閉じる。
                let span = tracing::info_span!("window.sync");
                state.pending.lock().expect("pending").insert(
                    seq,
                    Pending {
                        span,
                        sent_wall_us: wall_us(),
                    },
                );
                if mode == SYNC_CHANNEL {
                    if let Some(ch) = state.channel.lock().expect("channel").as_ref() {
                        let _ = ch.send(tick);
                    }
                } else if mode == SYNC_EMIT {
                    let _ = app.emit("playhead", tick);
                }
                // 戻ってこなかった古い通知は捨てる（止めた直後など）。
                let mut p = state.pending.lock().expect("pending");
                if p.len() > 600 {
                    p.retain(|k, _| *k + 300 > seq);
                }
            }
        })
        .expect("sync thread");
}

#[tauri::command]
pub fn subscribe(state: State<'_, Arc<SyncState>>, on_tick: Channel<Tick>) {
    *state.channel.lock().expect("channel") = Some(on_tick);
}

#[tauri::command]
pub async fn sync_ack(
    state: State<'_, Arc<SyncState>>,
    seq: u64,
    recv_us: u64,
    raf_us: u64,
) -> Result<(), String> {
    let back = wall_us();
    let Some(p) = state.pending.lock().expect("pending").remove(&seq) else {
        return Ok(());
    };
    drop(p.span);
    let s = &state.shared;
    let ms = |a: u64, b: u64| b.saturating_sub(a) as f64 / 1000.0;
    s.record("sync_send_to_webview_recv_ms", ms(p.sent_wall_us, recv_us));
    s.record("sync_send_to_raf_ms", ms(p.sent_wall_us, raf_us));
    s.record("sync_ack_return_ms", ms(raf_us, back));
    s.record("sync_roundtrip_ms", ms(p.sent_wall_us, back));
    Ok(())
}

#[tauri::command]
pub async fn key_event(
    state: State<'_, Arc<SyncState>>,
    source: String,
    key: String,
) -> Result<(), String> {
    keys::dispatch(&state.shared, &source, &key);
    Ok(())
}

#[tauri::command]
pub async fn ui_size(
    state: State<'_, Arc<SyncState>>,
    w: f64,
    h: f64,
    dpr: f64,
) -> Result<(), String> {
    *state.shared.ui_size.lock().expect("ui_size") = UiSize {
        width: w,
        height: h,
        dpr,
    };
    Ok(())
}
