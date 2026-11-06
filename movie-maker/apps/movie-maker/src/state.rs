//! ウィンドウの各スレッド（メイン、描画、再生、同期）が共有する状態と、壁時計の数字を貯める所。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use mm_runtime::{GpuContext, GpuFrame};
use mm_types::RgbaImage;
use mm_ui_layout::PointerEvent;
use serde::Serialize;

use crate::clock::PlayClock;
use crate::edit::PendingEdit;
use crate::playback::Command;

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

/// 再生スレッドから描画スレッドへ最新の 1 フレームを渡す箱。描画が遅れたときは古いフレームを捨てる。
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
    /// 再生中に音の時計に追いつけず飛ばしたフレームの数。
    pub dropped_frames: AtomicU64,
    /// 再生スレッドが描いたフレームの数。
    pub rendered_frames: AtomicU64,
    pub frames: FrameSlot,
    /// サーフェスが受けたポインターの入力。描画スレッドが取り出す。
    pub input: Mutex<Vec<PointerEvent>>,
    /// 再生スレッドへの命令。再生スレッドが立ち上がるまでは `None`。
    pub commands: Mutex<Option<Sender<Command>>>,
    /// 再生の主の時計。再生スレッドが立ち上がったときに置く。
    pub clock: OnceLock<Arc<dyn PlayClock>>,
    /// 描画スレッドが選んだアダプターの名前とバックエンド。
    pub adapter: Mutex<String>,
    /// 再生スレッドのレンダラーの GPU。描画スレッドはこのデバイスでサーフェスを作る。wgpu でなければ `None`。
    pub gpu: Mutex<Option<Arc<GpuContext>>>,
    /// 描画スレッドがレンダラーの GPU でサーフェスを作れず、CPU の画像を頼んでいるか。
    pub cpu_frames: AtomicBool,
    /// 再生スレッドがまだ当てていない操作。
    pub edits: Mutex<Vec<PendingEdit>>,
    series: Mutex<BTreeMap<&'static str, Vec<f64>>>,
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
            series: Mutex::new(BTreeMap::new()),
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

    /// 再生スレッドへ命令を送る。再生スレッドが無ければ捨てる。
    pub fn send(&self, command: Command) {
        if let Some(tx) = self.commands.lock().expect("commands").as_ref() {
            let _ = tx.send(command);
        }
    }

    /// 再生位置（秒）。時計が無ければ 0。
    pub fn playhead(&self) -> f64 {
        self.clock.get().map_or(0.0, |c| c.position())
    }

    /// 名前つきの系列に 1 つ足す。
    pub fn record(&self, name: &'static str, value: f64) {
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
