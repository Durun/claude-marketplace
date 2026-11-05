//! GPU のパスごとの時間。wgpu のタイムスタンプを 1 コマ分まとめて読み、`mm-trace` に載せる。
//!
//! 読み戻しで描画を止めないよう、1 コマ前の結果を次のコマの頭で読む。

use std::sync::Arc;

use mm_gpu::{map_blocking, GpuContext};

/// 1 コマに積めるパスの数。入れ子が深いとき超えた分は計らない。
const MAX_PASSES: u32 = 32;
const TIMESTAMP_BYTES: u64 = 8;

/// タイムスタンプのクエリ。`TIMESTAMP_QUERY` が無いアダプターでは何もしない。
pub struct GpuTimer {
    inner: Option<Inner>,
}

struct Inner {
    ctx: Arc<GpuContext>,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// 今のコマで使ったパスの数。
    used: u32,
    /// 読み戻し待ちのパスの数。
    pending: u32,
}

impl GpuTimer {
    /// 文脈がタイムスタンプを持つときだけ有効。
    pub fn new(ctx: &Arc<GpuContext>) -> Self {
        if !ctx.timestamps {
            return Self { inner: None };
        }
        let count = MAX_PASSES * 2;
        let bytes = u64::from(count) * TIMESTAMP_BYTES;
        let queries = ctx.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("mm-render timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count,
        });
        let buffer = |label, usage| {
            ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            inner: Some(Inner {
                ctx: ctx.clone(),
                queries,
                resolve: buffer(
                    "mm-render timestamps resolve",
                    wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                ),
                readback: buffer(
                    "mm-render timestamps readback",
                    wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                ),
                used: 0,
                pending: 0,
            }),
        }
    }

    /// 前のコマの結果を読み、GPU の時間（ナノ秒）を返す。読み戻し待ちが無ければ `None`。
    pub fn collect(&mut self) -> Option<u64> {
        let inner = self.inner.as_mut()?;
        if inner.pending == 0 {
            return None;
        }
        let pending = std::mem::take(&mut inner.pending);
        map_blocking(&inner.ctx.device, &inner.readback).ok()?;
        let data = inner.readback.slice(..).get_mapped_range().ok()?;
        let ticks: Vec<u64> = data
            .chunks_exact(TIMESTAMP_BYTES as usize)
            .take(pending as usize * 2)
            .map(|b| u64::from_le_bytes(b.try_into().expect("8 バイト")))
            .collect();
        drop(data);
        inner.readback.unmap();
        let total: u64 = ticks
            .chunks_exact(2)
            .map(|p| p[1].saturating_sub(p[0]))
            .sum();
        let period = f64::from(inner.ctx.queue.get_timestamp_period());
        Some((total as f64 * period) as u64)
    }

    /// 新しいコマの始まり。`collect` を呼んでから。
    pub fn begin_frame(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.used = 0;
        }
    }

    /// 次のパスの頭と尻に書くクエリ。埋まっていれば `None`。
    pub fn pass_writes(&mut self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let inner = self.inner.as_mut()?;
        if inner.used >= MAX_PASSES {
            return None;
        }
        let slot = inner.used * 2;
        inner.used += 1;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &inner.queries,
            beginning_of_pass_write_index: Some(slot),
            end_of_pass_write_index: Some(slot + 1),
        })
    }

    /// コマの最後に、積んだクエリを読み戻しの置き場へ写す。
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };
        if inner.used == 0 || inner.pending != 0 {
            return;
        }
        let count = inner.used * 2;
        let bytes = u64::from(count) * TIMESTAMP_BYTES;
        encoder.resolve_query_set(&inner.queries, 0..count, &inner.resolve, 0);
        encoder.copy_buffer_to_buffer(&inner.resolve, 0, &inner.readback, 0, bytes);
        inner.pending = inner.used;
    }
}
