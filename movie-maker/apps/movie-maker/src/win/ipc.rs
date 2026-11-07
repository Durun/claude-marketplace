//! Rust と WebView の間。再生位置を Rust から WebView へ送り（遅れを測る）、WebView の操作を受ける。
//!
//! 再生位置は Rust の時計が 1 つだけ持つ。WebView は受けた位置を、送られてから経った時間（壁時計の差）だけ
//! 進めて表示する。表示した値と本当の位置の差は ack で測って `webview_clock_error_ms` に貯める。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mm_doc::{Asset, Composition, Layer};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tracing::Span;

use crate::win::keys;
use crate::win::playback::Command;
use crate::win::state::{wall_us, OpenDoc, Shared};

/// パネルに属さない共通のコマンド（同期、再生の操作、編集の送信）。知らない名前には `false` を返す。
pub fn handler(invoke: tauri::ipc::Invoke) -> bool {
    let h: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![
        subscribe,
        sync_ack,
        transport,
        project_info,
        key_event,
        crate::win::edit::apply_op,
        crate::win::edit::apply_ops,
    ];
    h(invoke)
}

/// 通知の間隔（60 Hz）。
const TICK: Duration = Duration::from_nanos(16_666_667);
/// 止まっている間も位置を送り直す間隔。
const IDLE_RESEND: Duration = Duration::from_millis(250);
/// ack が戻らない通知を捨てる上限。
const PENDING_LIMIT: usize = 600;
/// 遅れの中央値を取る通知の数。
const LAG_WINDOW: usize = 30;

/// 再生位置の通知。
#[derive(Clone, Serialize)]
pub struct Tick {
    pub seq: u64,
    /// 再生位置（秒）。
    pub t: f64,
    /// 送った時刻（UNIX 時刻のマイクロ秒）。WebView は受けた時刻との差だけ位置を進める。
    pub sent_us: u64,
    pub playing: bool,
    /// 直近の「送ってから WebView の rAF まで」の中央値（ミリ秒）。
    pub lag_ms: f64,
}

/// レイヤー 1 本の情報（一覧に出す）。
#[derive(Clone, Serialize)]
pub struct LayerInfo {
    pub id: String,
    pub kind: String,
    pub start: f64,
    pub end: f64,
}

/// Timeline で選んだレイヤー。WebView の Inspector が受ける（`ui/shell` の `LayerSelection`）。
#[derive(Clone, Serialize)]
pub struct LayerSelection {
    pub layer: Option<Layer>,
    pub assets: Vec<Asset>,
}

/// WebView へ選択を送る Tauri のイベント名。`web/transport.js` が同じ名前の DOM のイベントにつなぐ。
const LAYER_SELECTED_EVENT: &str = "mm:layer-selected";

/// レイヤーの一覧（上のレイヤーが先頭。時間軸が壊れたレイヤーはスキップする）。
pub fn layer_infos(comp: &Composition) -> Vec<LayerInfo> {
    comp.layers
        .iter()
        .rev()
        .filter_map(|l| {
            let r = l.range().ok()?;
            Some(LayerInfo {
                id: l.id.to_string(),
                kind: l.source.kind.clone(),
                start: r.start.to_f64(),
                end: r.end.to_f64(),
            })
        })
        .collect()
}

/// 選んだレイヤーの ID から、Inspector に送る中身を作る。レイヤーが無くなっていれば空の選択。
pub fn selection_of(doc: &OpenDoc, selected: Option<&str>) -> LayerSelection {
    let layer = selected.and_then(|id| {
        let comp = doc.project.composition(&doc.comp).ok()?;
        comp.layers.iter().find(|l| l.id.as_str() == id).cloned()
    });
    LayerSelection {
        layer,
        assets: doc.project.assets.clone(),
    }
}

/// 開いたプロジェクトの情報。
#[derive(Clone, Serialize)]
pub struct ProjectInfo {
    pub name: String,
    pub composition: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub clock: String,
    pub audio_device: Option<String>,
    pub backends: Vec<(String, String)>,
    pub layers: Vec<LayerInfo>,
    pub adapter: String,
}

struct Pending {
    _span: Span,
    sent_wall_us: u64,
    /// 送った時点で再生中だったか。
    playing: bool,
}

/// 同期の状態。Tauri の `manage` に載せる。
pub struct SyncState {
    pub(crate) shared: Arc<Shared>,
    info: Mutex<Option<ProjectInfo>>,
    channel: Mutex<Option<Channel<Tick>>>,
    pending: Mutex<HashMap<u64, Pending>>,
    lags: Mutex<VecDeque<f64>>,
}

impl SyncState {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            info: Mutex::new(None),
            channel: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            lags: Mutex::new(VecDeque::new()),
        }
    }

    /// 開いたプロジェクトの情報を置く。
    pub fn set_info(&self, info: ProjectInfo) {
        *self.info.lock().expect("info") = Some(info);
    }

    /// 開いたプロジェクトの情報。レイヤーの一覧と fps、長さ、大きさは、いまのプロジェクトのデータから作る。
    fn current_info(&self) -> Option<ProjectInfo> {
        let mut info = self.info.lock().expect("info").clone()?;
        if let Some(doc) = self.shared.doc() {
            if let Ok(c) = doc.project.composition(&doc.comp) {
                info.layers = layer_infos(c);
                info.fps = c.fps.to_f64();
                info.duration = c.duration.to_f64();
                (info.width, info.height) = (c.width, c.height);
            }
        }
        info.adapter = self.shared.adapter.lock().expect("adapter").clone();
        Some(info)
    }

    fn lag_median(&self) -> f64 {
        let lags = self.lags.lock().expect("lags");
        let mut v: Vec<f64> = lags.iter().copied().collect();
        v.sort_by(|a, b| a.total_cmp(b));
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    }

    fn remember_lag(&self, ms: f64) {
        let mut lags = self.lags.lock().expect("lags");
        lags.push_back(ms);
        while lags.len() > LAG_WINDOW {
            lags.pop_front();
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
            let mut last: (f64, bool, Instant) = (f64::NAN, false, Instant::now());
            let mut sent_selection = (0, 0);
            let mut sent_source = None;
            let mut meters_playing = false;
            while !state.shared.quit.load(Ordering::SeqCst) {
                deadline += TICK;
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                } else {
                    deadline = now;
                }
                emit_selection_if_changed(&app, &state.shared, &mut sent_selection);
                crate::win::source_ipc::emit_if_changed(&app, &state.shared, &mut sent_source);
                let t = state.shared.playhead();
                crate::win::mixer_ipc::emit_meters(&app, &state.shared, t, &mut meters_playing);
                let playing = state.shared.playing.load(Ordering::SeqCst);
                let changed = playing || last.1 || (t - last.0).abs() > 1e-9;
                if !changed && last.2.elapsed() < IDLE_RESEND {
                    continue;
                }
                last = (t, playing, Instant::now());
                seq += 1;
                send_tick(&state, seq, t, playing);
            }
        })
        .expect("sync thread");
}

/// 選択かプロジェクトのデータが変わっていたら、選んだレイヤーの最新の中身を WebView に送る。
fn emit_selection_if_changed(app: &AppHandle, shared: &Shared, sent: &mut (u64, u64)) {
    let now = (
        shared.selection_serial.load(Ordering::SeqCst),
        shared.doc_serial.load(Ordering::SeqCst),
    );
    if now == *sent {
        return;
    }
    *sent = now;
    let Some(doc) = shared.doc() else { return };
    let payload = selection_of(&doc, shared.selected_layer().as_deref());
    let _ = app.emit(LAYER_SELECTED_EVENT, payload);
}

fn send_tick(state: &SyncState, seq: u64, t: f64, playing: bool) {
    let sent_us = wall_us();
    let tick = Tick {
        seq,
        t,
        sent_us,
        playing,
        lag_ms: state.lag_median(),
    };
    // span は ack が戻って pending から外れたときに閉じる。
    let mut pending = state.pending.lock().expect("pending");
    pending.insert(
        seq,
        Pending {
            _span: tracing::info_span!("window.sync"),
            sent_wall_us: sent_us,
            playing,
        },
    );
    if pending.len() > PENDING_LIMIT {
        pending.retain(|k, _| *k + 300 > seq);
    }
    drop(pending);
    if let Some(ch) = state.channel.lock().expect("channel").as_ref() {
        let _ = ch.send(tick);
    }
}

#[tauri::command]
pub fn subscribe(state: State<'_, Arc<SyncState>>, on_tick: Channel<Tick>) {
    *state.channel.lock().expect("channel") = Some(on_tick);
}

/// WebView が通知を受けて画面に出した（rAF）ときの報告。
#[tauri::command]
pub async fn sync_ack(
    state: State<'_, Arc<SyncState>>,
    seq: u64,
    recv_us: u64,
    raf_us: u64,
    shown_t: f64,
) -> Result<(), String> {
    let back = wall_us();
    let Some(p) = state.pending.lock().expect("pending").remove(&seq) else {
        return Ok(());
    };
    let s = &state.shared;
    let ms = |a: u64, b: u64| b.saturating_sub(a) as f64 / 1000.0;
    state.remember_lag(ms(p.sent_wall_us, raf_us));
    s.record("sync_send_to_webview_recv_ms", ms(p.sent_wall_us, recv_us));
    s.record("sync_send_to_raf_ms", ms(p.sent_wall_us, raf_us));
    s.record("sync_roundtrip_ms", ms(p.sent_wall_us, back));
    if p.playing && s.playing.load(Ordering::SeqCst) {
        // 本当の位置は、いまの位置から ack が戻るまでの時間を引いた rAF の時点の値。
        let at_raf = s.playhead() - ms(raf_us, back) / 1000.0;
        s.record("webview_clock_error_ms", (shown_t - at_raf) * 1000.0);
    }
    Ok(())
}

/// 再生の操作。`action` は play、pause、toggle、seek、step、home、end。
#[tauri::command]
pub async fn transport(
    state: State<'_, Arc<SyncState>>,
    action: String,
    value: Option<f64>,
) -> Result<(), String> {
    let command = match (action.as_str(), value) {
        ("play", _) => Command::Play,
        ("pause", _) => Command::Pause,
        ("toggle", _) => Command::Toggle,
        ("seek", Some(t)) => Command::Seek(t),
        ("step", Some(n)) => Command::Step(n as i64),
        ("home", _) => Command::Seek(0.0),
        ("end", _) => Command::Seek(f64::MAX),
        _ => return Err(format!("知らない再生の操作: {action}")),
    };
    state.shared.send(command);
    Ok(())
}

#[tauri::command]
pub async fn project_info(state: State<'_, Arc<SyncState>>) -> Result<ProjectInfo, String> {
    state
        .current_info()
        .ok_or_else(|| "プロジェクトを開いていない".to_owned())
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
