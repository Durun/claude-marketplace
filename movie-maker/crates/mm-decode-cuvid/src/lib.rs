//! FFmpeg の cuvid（NVDEC）で復号した CUDA のメモリを、D3D12 の共有バッファへ 1 回コピーし、
//! 描画器と同じ wgpu のデバイスの上で線形の `Rgba16Float` にして返すデコーダー。
//! `d3d12va` が開いた直後に失敗する素材（NVENC で作った 4K の HEVC など）の逃げ道。
//!
//! - デバイスは `DecoderFactory::open` で渡される描画器のもの。CUDA のデバイスは、そのアダプターの LUID で選ぶ。
//! - CUDA のコピーと wgpu の読み取りは、枠ごとの共有フェンスを交互に進めて順序づける。
//! - 10 bit の素材（P010）は 8 bit に落とさず、16 bit のまま読む。
//!
//! FFmpeg、CUDA、D3D12 に触る所は feature `cuvid`（既定で切る）の中にある。切り替え方は README。
//! 結果は `docs/design/spikes/nvdec.md`。

/// 実装の名前。`DecoderFactory::id` に使う名前と同じ。
pub const ID: &str = "cuvid";

/// この組み立てで cuvid の経路が入っているか。
pub const ENABLED: bool = cfg!(feature = "cuvid");

#[cfg(feature = "cuvid")]
mod convert;
#[cfg(feature = "cuvid")]
pub mod cuda;
#[cfg(feature = "cuvid")]
mod dec;
#[cfg(feature = "cuvid")]
mod decoder;
#[cfg(feature = "cuvid")]
mod hooks;
#[cfg(feature = "cuvid")]
mod share;

#[cfg(feature = "cuvid")]
pub use decoder::{gpu_request, CuvidDecoder, CuvidFactory};
/// 検査のための故障の注入と資源の数え（feature `test-hooks`）。
#[cfg(all(feature = "cuvid", feature = "test-hooks"))]
pub use hooks::{inject, live_imports, Fault};
