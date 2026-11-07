//! ウィンドウの各スレッド（メイン、描画、再生、同期）が共有する状態と、壁時計の数字を貯める所。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use mm_doc::Project;
use mm_runtime::{GpuContext, GpuFrame};
use mm_types::{CompositionId, RgbaImage};
use mm_ui_layout::PointerEvent;
use serde::Serialize;

use crate::win::clock::PlayClock;
use crate::win::edit::{EditJob, PendingEdit};
use crate::win::history::HistoryMirror;
use crate::win::playback::Command;
use crate::win::sequence_edit::EditState;
use crate::win::source::SourceSlot;

/// 百分位つきの要約（単位は呼び出し側の決め。ここではミリ秒）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub min: f64,
}

/// 値の列から要約を作る。
pub fn summarize(values: &[f64]) -> Summary {
    if values.is_empty() {
        return Summary::default();
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| v[(((v.len() - 1) as f64) * q).round() as usize];
    Summary {
        n: v.len(),
        mean: v.iter().sum::<f64>() / v.len() as f64,
        p50: at(0.50),
        p95: at(0.95),
        p99: at(0.99),
        max: v[v.len() - 1],
        min: v[0],
    }
}

/// フレームの中身。wgpu のレンダラーは GPU のテクスチャのまま渡し、描画スレッドが同じデバイスで描く。
#[derive(Clone)]
pub enum FrameImage {
    /// CPU の画像（wgpu でないレンダラー）。描画スレッドが上げる。
    Cpu(Arc<RgbaImage>),
    /// 再生スレッドのレンダラーが描いたテクスチャ。
    Gpu(GpuFrame),
}

/// 再生スレッドが描いた 1 フレーム。
#[derive(Clone)]
pub struct PreviewFrame {
    /// 通し番号。描画側が新しいかどうかの判定に使う。
    pub serial: u64,
    /// コンポジションの時間軸での時刻（秒）。
    pub time: f64,
    pub image: FrameImage,
}

/// 再生スレッドから描画スレッドへ最新の 1 フレームを渡すスロット。描画が遅れたときは古いフレームを捨てる。
#[derive(Default)]
pub struct FrameSlot {
    slot: Mutex<Option<PreviewFrame>>,
    serial: AtomicU64,
}

impl FrameSlot {
    /// 新しいフレームを置く。
    pub fn put(&self, time: f64, image: FrameImage) {
        let serial = self.serial.fetch_add(1, Ordering::SeqCst) + 1;
        *self.slot.lock().expect("frame slot") = Some(PreviewFrame {
            serial,
            time,
            image,
        });
    }

    /// `seen` より新しいフレームがあれば返す。
    pub fn newer_than(&self, seen: u64) -> Option<PreviewFrame> {
        let guard = self.slot.lock().expect("frame slot");
        guard.as_ref().filter(|f| f.serial > seen).cloned()
    }
}

/// いま開いているプロジェクトのデータとコンポジション。操作を適用するたびに置き直す。
#[derive(Clone)]
pub struct OpenDoc {
    pub project: Arc<Project>,
    pub comp: CompositionId,
    /// `project.mmdb` のバージョン。編集の計画に付けて、間に別の発行元が触った所との衝突を見分ける。
    /// JSON のプロジェクトでは `None`。
    pub version: Option<i64>,
}

/// アプリ全体で 1 つの共有状態。
pub struct Shared {
    /// 全スレッドを止める合図。
    pub quit: AtomicBool,
    /// サーフェスの物理ピクセルの大きさ。上位 32 bit が幅、下位 32 bit が高さ。
    surface_size: AtomicU64,
    pub frames_presented: AtomicU64,
    pub surface_errors: AtomicU64,
    /// 再生中か。再生スレッドが書く。
    pub playing: AtomicBool,
    /// 再生中に音の時計に追いつけずドロップしたフレームの数。
    pub dropped_frames: AtomicU64,
    /// 再生スレッドが描いたフレームの数。
    pub rendered_frames: AtomicU64,
    pub frames: FrameSlot,
    /// サーフェスが受けたポインターの入力。描画スレッドが取り出す。
    pub input: Mutex<Vec<PointerEvent>>,
    /// 再生スレッドへのコマンド。再生スレッドが立ち上がるまでは `None`。
    pub commands: Mutex<Option<Sender<Command>>>,
    /// 再生の主の時計。再生スレッドが立ち上がったときに置く。
    pub clock: OnceLock<Arc<dyn PlayClock>>,
    /// 描画スレッドが選んだアダプターの名前とバックエンド。
    pub adapter: Mutex<String>,
    /// 再生スレッドのレンダラーの GPU。描画スレッドはこのデバイスでサーフェスを作る。wgpu でなければ `None`。
    pub gpu: Mutex<Option<Arc<GpuContext>>>,
    /// 描画スレッドがレンダラーの GPU でサーフェスを作れず、CPU の画像を頼んでいるか。
    pub cpu_frames: AtomicBool,
    /// 再生スレッドがまだ適用していない操作。
    pub edits: Mutex<Vec<PendingEdit>>,
    /// キーとドラッグの編集を順に計画して適用するキュー。最初の編集で作る。
    pub edit_lane: Mutex<Option<Sender<EditJob>>>,
    /// 書き手として開いた `project.mmdb` の中心。操作はここで保存してから描き直す。JSON のプロジェクトでは空。
    pub core: OnceLock<Arc<mm_serve::Core>>,
    /// `core` の変更で触った ID のうち、再生スレッドがまだキャッシュを捨てていないもの。
    pub reload_touched: Mutex<Vec<String>>,
    /// Review パネルが Program に当てて見せている提案。
    pub review_preview: crate::win::review::PreviewSlot,
    /// `core` の取り消しのスタックのコピー。取り消しの文言に、戻したトランザクションの発行元を出すのに使う。
    pub history: HistoryMirror,
    /// いま開いているプロジェクトのデータ。Timeline の表示と WebView のレイヤーの一覧はここから作る。
    doc: Mutex<Option<OpenDoc>>,
    /// `doc` を置き直すたびに進む。読む側が作り直すかどうかの判定に使う。
    pub doc_serial: AtomicU64,
    /// Timeline で選んだレイヤーの ID。
    selected_layer: Mutex<Option<String>>,
    /// 選択を置き直すたびに進む。
    pub selection_serial: AtomicU64,
    /// Timeline のカットの選択と、編集の結果の文言。
    pub edit: EditState,
    /// Source のビューアー。
    pub source: SourceSlot,
    /// 計測の系列を貯めるか。自動試験のときだけ貯め、普段の起動で増え続けないようにする。
    collect_series: AtomicBool,
    series: Mutex<BTreeMap<&'static str, Vec<f64>>>,
    /// Mixer のメーター。再生スレッドが音を混ぜるたびに記録し、同期のスレッドが送る。
    pub meters: Mutex<crate::win::mixer::MeterLane>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            quit: AtomicBool::new(false),
            surface_size: AtomicU64::new(0),
            frames_presented: AtomicU64::new(0),
            surface_errors: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            dropped_frames: AtomicU64::new(0),
            rendered_frames: AtomicU64::new(0),
            frames: FrameSlot::default(),
            input: Mutex::new(Vec::new()),
            commands: Mutex::new(None),
            clock: OnceLock::new(),
            adapter: Mutex::new(String::new()),
            gpu: Mutex::new(None),
            cpu_frames: AtomicBool::new(false),
            edits: Mutex::new(Vec::new()),
            edit_lane: Mutex::new(None),
            core: OnceLock::new(),
            reload_touched: Mutex::new(Vec::new()),
            review_preview: crate::win::review::PreviewSlot::default(),
            history: HistoryMirror::default(),
            doc: Mutex::new(None),
            doc_serial: AtomicU64::new(0),
            selected_layer: Mutex::new(None),
            selection_serial: AtomicU64::new(0),
            edit: EditState::default(),
            source: SourceSlot::default(),
            collect_series: AtomicBool::new(false),
            series: Mutex::new(BTreeMap::new()),
            meters: Mutex::new(crate::win::mixer::MeterLane::default()),
        }
    }

    pub fn set_surface_size(&self, w: u32, h: u32) {
        self.surface_size
            .store(((w as u64) << 32) | h as u64, Ordering::SeqCst);
    }

    pub fn surface_size(&self) -> (u32, u32) {
        let v = self.surface_size.load(Ordering::SeqCst);
        ((v >> 32) as u32, v as u32)
    }

    /// 再生スレッドへコマンドを送る。再生スレッドが無ければ捨てる。
    pub fn send(&self, command: Command) {
        if let Some(tx) = self.commands.lock().expect("commands").as_ref() {
            let _ = tx.send(command);
        }
    }

    /// 再生位置（秒）。時計が無ければ 0。
    pub fn playhead(&self) -> f64 {
        self.clock.get().map_or(0.0, |c| c.position())
    }

    /// 開いているプロジェクトのデータを置き直し、読む側に知らせる。
    pub fn publish_doc(&self, project: Arc<Project>, comp: CompositionId) {
        *self.doc.lock().expect("doc") = Some(OpenDoc {
            project,
            comp,
            version: None,
        });
        self.doc_serial.fetch_add(1, Ordering::SeqCst);
    }

    /// `project.mmdb` のバージョン `version` のデータを置く。置いてあるものより新しいときだけ置き直し、置いたら真。
    /// 保存した直後の読み直しと変更の通知は別のスレッドから来るので、古いほうで上書きしない。
    pub fn publish_saved(&self, version: i64, project: Arc<Project>, comp: CompositionId) -> bool {
        let mut doc = self.doc.lock().expect("doc");
        if doc.as_ref().and_then(|d| d.version) >= Some(version) {
            return false;
        }
        *doc = Some(OpenDoc {
            project,
            comp,
            version: Some(version),
        });
        self.doc_serial.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// いま開いているプロジェクトのデータ。
    pub fn doc(&self) -> Option<OpenDoc> {
        self.doc.lock().expect("doc").clone()
    }

    /// Timeline で選んだレイヤーを置く。外すときは `None`。
    pub fn select_layer(&self, id: Option<String>) {
        let mut selected = self.selected_layer.lock().expect("selected");
        if *selected != id {
            *selected = id;
            self.selection_serial.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// 選んでいるレイヤーの ID。
    pub fn selected_layer(&self) -> Option<String> {
        self.selected_layer.lock().expect("selected").clone()
    }

    /// 計測の系列を貯め始める（自動試験用）。
    pub fn collect_series(&self) {
        self.collect_series.store(true, Ordering::SeqCst);
    }

    /// 名前つきの系列に 1 つ足す。貯める設定でなければ捨てる。
    pub fn record(&self, name: &'static str, value: f64) {
        if !self.collect_series.load(Ordering::SeqCst) {
            return;
        }
        self.series
            .lock()
            .expect("series lock")
            .entry(name)
            .or_default()
            .push(value);
    }

    /// 系列を全部取り出して空にする。
    pub fn drain_series(&self) -> BTreeMap<&'static str, Summary> {
        let taken = std::mem::take(&mut *self.series.lock().expect("series lock"));
        taken.into_iter().map(|(k, v)| (k, summarize(&v))).collect()
    }
}

/// 壁時計（UNIX 時刻）のマイクロ秒。WebView の `performance.timeOrigin + now()` と同じ時計。
pub fn wall_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_types::{Rgba, Size};

    #[test]
    fn 要約は百分位と最大を返す() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = summarize(&v);
        assert_eq!((s.n, s.max, s.min), (100, 100.0, 1.0));
        assert!((s.p50 - 50.0).abs() <= 1.0);
        assert!((s.p99 - 99.0).abs() <= 1.0);
    }

    #[test]
    fn 系列は貯める設定にしたときだけ増える() {
        let s = Shared::new();
        s.record("x", 1.0);
        assert!(s.drain_series().is_empty(), "普段の起動では貯めない");
        s.collect_series();
        s.record("x", 1.0);
        assert_eq!(s.drain_series()["x"].n, 1);
    }

    #[test]
    fn 選択は変わったときだけ通し番号が進む() {
        let s = Shared::new();
        s.select_layer(Some("a".into()));
        s.select_layer(Some("a".into()));
        assert_eq!(s.selection_serial.load(Ordering::SeqCst), 1);
        s.select_layer(None);
        assert_eq!(
            (
                s.selected_layer(),
                s.selection_serial.load(Ordering::SeqCst)
            ),
            (None, 2)
        );
    }

    #[test]
    fn サーフェスの大きさは幅と高さを詰めて持つ() {
        let s = Shared::new();
        s.set_surface_size(1536, 960);
        assert_eq!(s.surface_size(), (1536, 960));
    }

    #[test]
    fn 描画側は見ていない新しいフレームだけ受け取る() {
        let slot = FrameSlot::default();
        let img = FrameImage::Cpu(Arc::new(RgbaImage::filled(Size::new(2, 2), Rgba::WHITE)));
        assert!(slot.newer_than(0).is_none());
        slot.put(0.5, img.clone());
        let f = slot.newer_than(0).expect("新しい");
        assert_eq!((f.serial, f.time), (1, 0.5));
        assert!(slot.newer_than(f.serial).is_none());
        slot.put(0.6, img);
        assert_eq!(slot.newer_than(f.serial).expect("次").serial, 2);
    }
}
