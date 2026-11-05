//! 試作: FFmpeg の cuvid（NVDEC）で復号した CUDA のメモリを、D3D12 の共有バッファへ 1 回コピーして wgpu に渡す。
//!
//! FFmpeg、CUDA、D3D12 に触る所は feature `cuvid`（既定で切る）の中にある。切り替え方は README。
//! 結果は `docs/design/spikes/nvdec.md`。試作は `Decoder` の実装までは行かず、bench で経路の可否と時間を測る。

/// 実装の名前。`DecoderFactory::id` に使う名前と同じ。
pub const ID: &str = "cuvid";

/// この組み立てで cuvid の経路が入っているか。
pub const ENABLED: bool = cfg!(feature = "cuvid");

#[cfg(feature = "cuvid")]
pub mod convert;
#[cfg(feature = "cuvid")]
pub mod cuda;
#[cfg(feature = "cuvid")]
pub mod dec;
#[cfg(feature = "cuvid")]
pub mod gpu;
#[cfg(feature = "cuvid")]
pub mod share;
