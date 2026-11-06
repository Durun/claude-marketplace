//! FFmpeg の d3d12va でデコードした NV12 / P010 の `ID3D12Resource` を、コピーなしで wgpu のテクスチャにラップし、
//! レンダラーと同じデバイスの上で線形の `Rgba16Float` に変えて返すデコーダー。
//!
//! - デバイスは `DecoderFactory::open` で渡されるレンダラーのもの（`mm-gpu` の `GpuContext`）。自前では作らない。
//! - デコードしたサーフェスは、変換を読み終えたことを wgpu のキューのフェンスで確かめるまで FFmpeg のプールに返さない。
//! - 10 bit の素材（P010）は 8 bit に下げず、`R16Unorm` と `Rg16Unorm` のサーフェスで読む。
//!
//! FFmpeg と D3D12 に触る所は feature `d3d12va`（既定で切る）の中にある。切り替え方は README。
//! 結果は `docs/design/spikes/nvdec.md`。

pub mod ffi;

/// 実装の名前。`DecoderFactory::id` と同じ。
pub const ID: &str = "d3d12va";

/// この組み立てで d3d12va の経路が入っているか。
pub const ENABLED: bool = cfg!(feature = "d3d12va");

#[cfg(feature = "d3d12va")]
mod convert;
#[cfg(feature = "d3d12va")]
mod decoder;
#[cfg(feature = "d3d12va")]
mod hwdec;
#[cfg(feature = "d3d12va")]
mod probe;
#[cfg(feature = "d3d12va")]
mod wrap;

#[cfg(feature = "d3d12va")]
pub use decoder::{gpu_request, required_features, D3d12vaDecoder, D3d12vaFactory};
#[cfg(feature = "d3d12va")]
pub use probe::{PROBE_ENV, PROBE_FRAMES};
