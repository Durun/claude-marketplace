//! 描いたフレームのキャッシュ。ページ、パラメータ、時刻、大きさ、倍率の組で引く。
//! 静止のページは時刻を 0 に揃えるので、パラメータの組ごとに 1 枚だけ描けば済む。

use std::sync::{Arc, Weak};

use mm_gpu::GpuContext;
use mm_types::{RationalTime, RgbaImage, Size};

use crate::page::FrameRequest;

/// キャッシュするフレームの数。1080p の `Rgba16Float` は 1 枚 16 MB なので、静止の部品が数個並ぶ程度に留める。
/// 動くページの全フレームはレンダラーのキャッシュが持つ。
const KEPT_FRAMES: usize = 6;

/// 画素を決める入力の組。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameKey {
    page: u64,
    data: String,
    time: RationalTime,
    size: Size,
    scale_bits: u32,
}

impl FrameKey {
    /// 頼みから。
    pub fn of(req: &FrameRequest<'_>) -> Self {
        Self {
            page: req.page.hash,
            data: req.page.data_text.clone(),
            time: req.time,
            size: req.size,
            scale_bits: req.scale.to_bits(),
        }
    }
}

/// キャッシュしたフレーム。
#[derive(Clone)]
pub enum Cached {
    /// 中間の形式のテクスチャ。作ったデバイスが作り直されたら使わない。
    Gpu {
        texture: wgpu::Texture,
        ctx: Weak<GpuContext>,
    },
    /// CPU の画像。
    Cpu(Arc<RgbaImage>),
}

/// 新しい順の表。
#[derive(Default)]
pub struct FrameCache {
    entries: Vec<(FrameKey, Cached)>,
}

impl FrameCache {
    /// 引く。ヒットしたものは先頭へ動かす。
    pub fn get(&mut self, key: &FrameKey) -> Option<Cached> {
        let i = self.entries.iter().position(|(k, _)| k == key)?;
        let hit = self.entries.remove(i);
        let value = hit.1.clone();
        self.entries.insert(0, hit);
        Some(value)
    }

    /// キャッシュする。上限を超えたら古いものから捨てる。
    pub fn put(&mut self, key: FrameKey, value: Cached) {
        self.entries.retain(|(k, _)| *k != key);
        self.entries.insert(0, (key, value));
        self.entries.truncate(KEPT_FRAMES);
    }
}

impl std::fmt::Debug for FrameCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameCache")
            .field("len", &self.entries.len())
            .finish()
    }
}
