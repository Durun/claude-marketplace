//! 組み立て。実装を選ぶのはこのクレートだけで、入口（アプリ、CLI、サーバー）は [`Engine`] を受け取る。
//!
//! 設定の各欄は見本（`sample`）か本物（`wgpu`、`glyphon`、`dsp`、`ffmpeg`）を選ぶ。
//! `ffmpeg` は feature `ffmpeg` のときだけ組める。本物の実装が増えたら [`assemble`] に腕を足す（口は変えない）。

mod assemble;
mod config;
mod engine;
mod registry;

pub use assemble::assemble;
pub use config::{Backend, RuntimeConfig};
pub use engine::{Engine, EngineParts, ExportReport, EXPORT_BATCH_FRAMES};
pub use registry::Registry;
