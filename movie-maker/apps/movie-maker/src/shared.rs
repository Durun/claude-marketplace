//! 窓の各スレッド（メイン、描画、同期、自己診断）が共有する状態と、壁時計の数字を貯める所。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// 同期を止めた状態。
pub const SYNC_OFF: u8 = 0;
/// `tauri::ipc::Channel` で再生位置を送る。
pub const SYNC_CHANNEL: u8 = 1;
/// `emit`（イベント）で再生位置を送る。
pub const SYNC_EMIT: u8 = 2;

/// 百分位つきの要約（単位は呼び出し側の決め。ここではミリ秒）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
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
    }
}

/// 窓の外の WebView が最後に報告した大きさ。
#[derive(Debug, Clone, Copy, Default)]
pub struct UiSize {
    pub width: f64,
    pub height: f64,
    pub dpr: f64,
}

/// アプリ全体で 1 つの共有状態。
pub struct Shared {
    pub t0: Instant,
    /// 面の物理ピクセルの大きさ。上位 32 bit が幅、下位 32 bit が高さ。
    pub surface_size: AtomicU64,
    /// 最後に配置を変えた時刻（`t0` からのマイクロ秒）。描画側が最初の present で 0 に戻す。
    pub layout_us: AtomicU64,
    /// 面にキーが届いた時刻（`t0` から）。見た目の確認で面を光らせる。
    pub flash_us: AtomicU64,
    /// 自己診断が `SendInput` を呼んだ時刻（`t0` から）。0 なら測らない。
    pub input_stamp_us: AtomicU64,
    pub sync_mode: AtomicU8,
    pub frames: AtomicU64,
    pub surface_errors: AtomicU64,
    pub keys_surface: AtomicU64,
    pub keys_webview: AtomicU64,
    pub ui_size: Mutex<UiSize>,
    /// 描画スレッドが選んだアダプターの名前とバックエンド。
    pub adapter: Mutex<String>,
    series: Mutex<BTreeMap<&'static str, Vec<f64>>>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            t0: Instant::now(),
            surface_size: AtomicU64::new(0),
            layout_us: AtomicU64::new(0),
            flash_us: AtomicU64::new(0),
            input_stamp_us: AtomicU64::new(0),
            sync_mode: AtomicU8::new(SYNC_OFF),
            frames: AtomicU64::new(0),
            surface_errors: AtomicU64::new(0),
            keys_surface: AtomicU64::new(0),
            keys_webview: AtomicU64::new(0),
            ui_size: Mutex::new(UiSize::default()),
            adapter: Mutex::new(String::new()),
            series: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn now_us(&self) -> u64 {
        self.t0.elapsed().as_micros() as u64
    }

    pub fn set_surface_size(&self, w: u32, h: u32) {
        self.surface_size
            .store(((w as u64) << 32) | h as u64, Ordering::SeqCst);
    }

    pub fn surface_size(&self) -> (u32, u32) {
        let v = self.surface_size.load(Ordering::SeqCst);
        ((v >> 32) as u32, v as u32)
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

    #[test]
    fn 要約は百分位と最大を返す() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = summarize(&v);
        assert_eq!(s.n, 100);
        assert_eq!(s.max, 100.0);
        assert!((s.p50 - 50.0).abs() <= 1.0);
        assert!((s.p99 - 99.0).abs() <= 1.0);
    }

    #[test]
    fn 面の大きさは幅と高さを詰めて持つ() {
        let s = Shared::new();
        s.set_surface_size(1536, 960);
        assert_eq!(s.surface_size(), (1536, 960));
    }
}
