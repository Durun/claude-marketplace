//! GPU のパスごとの時間。wgpu のタイムスタンプをフレーム単位でまとめ、数フレーム遅れて読む。
//!
//! 読む側が GPU の完了を待つと CPU と GPU が毎フレーム同期し、書き出しのリードバックの先行が重ならない。
//! そのためリードバックのバッファを数枚のリングバッファにして、`map_async` の完了を `Poll` で拾うだけにする。
//! 合成・`fill`・効果・NV12 の変換のどのパスも `pass_writes` で同じ問い合わせの集まりに書ける。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::context::GpuContext;

/// 1 フレームに積めるパスの数。超えた分は計らない。
const MAX_PASSES: u32 = 256;
/// 結果の読み取り待ちを抱えられるフレームの数。埋まっているフレームは計らない。
const SLOTS: usize = 3;
const TIMESTAMP_BYTES: u64 = 8;

/// 読み取りの状態。
const IDLE: u8 = 0;
const MAPPING: u8 = 1;
const MAPPED: u8 = 2;
const FAILED: u8 = 3;

/// 読み取り待ちの 1 フレーム分のバッファ。
struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

/// リングバッファの状態。
#[derive(Default)]
struct Ring {
    /// 提出した順の（スロット、パスの数）。
    pending: VecDeque<(usize, u32)>,
    /// 今のフレームの `resolve` で選んだスロット。提出の後に `map_async` する。
    staged: Option<(usize, u32)>,
}

struct Inner {
    ctx: Arc<GpuContext>,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    /// 次の `resolve` までに積んだパスの数。
    used: AtomicU32,
    ring: Mutex<Ring>,
}

/// タイムスタンプのクエリ。`TIMESTAMP_QUERY` が無いアダプターでは何もしない。
pub struct PassTimer {
    inner: Option<Inner>,
}

impl PassTimer {
    /// コンテキストがタイムスタンプを持つときだけ有効。
    pub fn new(ctx: &Arc<GpuContext>) -> Self {
        if !ctx.timestamps {
            return Self { inner: None };
        }
        let count = MAX_PASSES * 2;
        let bytes = u64::from(count) * TIMESTAMP_BYTES;
        let buffer = |label, usage| {
            ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: buffer(
                    "mm-gpu timestamps readback",
                    wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                ),
                state: Arc::new(AtomicU8::new(IDLE)),
            })
            .collect();
        Self {
            inner: Some(Inner {
                ctx: ctx.clone(),
                queries: ctx.device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("mm-gpu timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count,
                }),
                resolve: buffer(
                    "mm-gpu timestamps resolve",
                    wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                ),
                slots,
                used: AtomicU32::new(0),
                ring: Mutex::new(Ring::default()),
            }),
        }
    }

    /// 次のパスの開始と終了に書くクエリ。埋まっていれば `None`。
    pub fn pass_writes(&self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let inner = self.inner.as_ref()?;
        let n = inner.used.fetch_add(1, Ordering::Relaxed);
        if n >= MAX_PASSES {
            inner.used.store(MAX_PASSES, Ordering::Relaxed);
            return None;
        }
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &inner.queries,
            beginning_of_pass_write_index: Some(n * 2),
            end_of_pass_write_index: Some(n * 2 + 1),
        })
    }

    /// ここまでに積んだクエリを、空いている読み取りバッファへコピーするコマンドを足す。
    /// 空きが無い（GPU が遅れている）ときは、この回の分は計らない。続けて `after_submit` を呼ぶ。
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let used = inner.used.load(Ordering::Relaxed).min(MAX_PASSES);
        if used == 0 {
            return;
        }
        let mut ring = inner.lock();
        let Some(slot) = (0..SLOTS).find(|i| ring.pending.iter().all(|(p, _)| p != i)) else {
            return;
        };
        let bytes = u64::from(used * 2) * TIMESTAMP_BYTES;
        encoder.resolve_query_set(&inner.queries, 0..used * 2, &inner.resolve, 0);
        encoder.copy_buffer_to_buffer(&inner.resolve, 0, &inner.slots[slot].buffer, 0, bytes);
        ring.staged = Some((slot, used));
    }

    /// `resolve` を含むコマンドを提出した後に呼ぶ。読み取りを頼み、次のフレームの数え直しを始める。
    pub fn after_submit(&self) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        inner.used.store(0, Ordering::Relaxed);
        let mut ring = inner.lock();
        let Some((slot, used)) = ring.staged.take() else {
            return;
        };
        let state = inner.slots[slot].state.clone();
        state.store(MAPPING, Ordering::Release);
        inner.slots[slot]
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                state.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
            });
        ring.pending.push_back((slot, used));
    }

    /// 読み取りが終わった分の GPU の時間（ナノ秒）を、古い順に返す。待たない。
    pub fn collect(&self) -> Vec<u64> {
        self.collect_with(wgpu::PollType::Poll)
    }

    /// 頼んだ読み取りの完了を待ってから、全部の GPU の時間（ナノ秒）を古い順に返す。
    /// 描画を止めて結果を確定させたい所（試験、書き出しの終わり）が使う。
    pub fn collect_all(&self) -> Vec<u64> {
        self.collect_with(wgpu::PollType::wait_indefinitely())
    }

    fn collect_with(&self, poll: wgpu::PollType) -> Vec<u64> {
        let Some(inner) = self.inner.as_ref() else {
            return Vec::new();
        };
        let mut ring = inner.lock();
        if ring.pending.is_empty() {
            return Vec::new();
        }
        let _ = inner.ctx.device.poll(poll);
        let mut out = Vec::new();
        while let Some(&(slot, used)) = ring.pending.front() {
            match inner.slots[slot].state.load(Ordering::Acquire) {
                MAPPED => out.extend(inner.read(slot, used)),
                FAILED => {}
                _ => break,
            }
            inner.slots[slot].state.store(IDLE, Ordering::Release);
            ring.pending.pop_front();
        }
        out
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// マップした範囲から、`used` 個のパスの長さの合計を ns で返し、アンマップする。
    fn read(&self, slot: usize, used: u32) -> Option<u64> {
        let buffer = &self.slots[slot].buffer;
        let ticks: Option<Vec<u64>> = buffer.slice(..).get_mapped_range().ok().map(|data| {
            data.chunks_exact(TIMESTAMP_BYTES as usize)
                .take(used as usize * 2)
                .map(|b| u64::from_le_bytes(b.try_into().expect("8 バイト")))
                .collect()
        });
        buffer.unmap();
        let ticks = ticks?;
        let total: u64 = ticks
            .chunks_exact(2)
            .map(|p| p[1].saturating_sub(p[0]))
            .sum();
        let period = f64::from(self.ctx.queue.get_timestamp_period());
        Some((total as f64 * period) as u64)
    }
}
