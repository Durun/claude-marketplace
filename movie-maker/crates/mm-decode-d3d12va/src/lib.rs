//! 試作: FFmpeg の d3d12va で復号した NV12 の `ID3D12Resource` を、コピーなしで wgpu のテクスチャに包む。
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
mod gpu;
#[cfg(feature = "d3d12va")]
mod hwdec;
#[cfg(feature = "d3d12va")]
mod wrap;

#[cfg(feature = "d3d12va")]
pub use convert::{Converter, CpuUploader};
#[cfg(feature = "d3d12va")]
pub use decoder::{D3d12vaDecoder, D3d12vaFactory, HeldFrame, HeldFrameRef};
#[cfg(feature = "d3d12va")]
pub use gpu::GpuContext;
#[cfg(feature = "d3d12va")]
pub use hwdec::{init, HwDecoder};
#[cfg(feature = "d3d12va")]
pub use wrap::{GpuNv12, Wrapper};
