//! 組み立て。実装を選ぶのはこのクレートだけで、操作経路（アプリ、CLI、サーバー）は [`Engine`] を受け取る。
//!
//! 設定の各フィールドはモック（`sample`）か実際の実装（`wgpu`、`glyphon`、`dsp`、`ffmpeg`）を選ぶ。
//! `ffmpeg` は feature `ffmpeg`、`d3d12va` と `cuvid` は同名の feature のときだけ組める。`auto` は素材ごとに d3d12va、cuvid、ffmpeg の順に試す。実際の実装が増えたら [`assemble`] に腕を足す（インターフェースは変えない）。

mod assemble;
mod config;
mod decode_select;
mod encode_select;
mod engine;
mod registry;

pub use assemble::assemble;
pub use config::{Backend, RuntimeConfig};
pub use decode_select::{AutoDecoderFactory, GpuCandidate, SplitDecoder, WithAudio};
pub use encode_select::AutoEncoderFactory;
pub use engine::{Engine, EngineParts, GpuFrame};
pub use mm_gpu::GpuContext;
pub use mm_render_api::TrackLevel;
pub use registry::Registry;
