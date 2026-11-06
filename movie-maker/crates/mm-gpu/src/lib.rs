//! GPU を触る実装の共有部（RFC 0001）。口のクレートは wgpu を出さず、実装どうしはここで wgpu の型を合わせる。
//!
//! - [`GpuContext`]: Device と Queue。アダプターを 1 回選び、レンダラー・デコーダー・ホストで共有する。
//! - [`WgpuGpu`]: 口の `GpuHelper` の wgpu 実装。種類は `as_any` で降りてテクスチャを使う。
//! - [`GpuFrameTexture`]: デコーダーが返す GPU のフレームの中身。`GpuHelper::import` でレンダラーが受ける。
//!
//! 依存の規則では「実装の共有部」で、口（`mm-*-api`）はこれに依存せず、GPU を触る実装だけが依存する。

mod context;
mod frame;
mod helper;

pub use context::{adapter_id, enumerate_adapters, GpuContext, GpuRequest, FRAME_FORMAT};
pub use frame::{GpuFrameTexture, WaitFn};
pub use helper::{map_blocking, WgpuGpu};
