//! 組み立て。実装を選ぶのはこのクレートだけで、入口（アプリ、CLI、サーバー）は [`Engine`] を受け取る。
//!
//! 今は見本の実装（`mm-testkit`）しか無いので、設定の各欄は `sample` だけを受け付ける。
//! 本物の実装のクレートが入ったら、[`assemble`] に選択肢を足す（口は変えない）。

mod assemble;
mod config;
mod engine;
mod registry;

pub use assemble::assemble;
pub use config::{Backend, RuntimeConfig};
pub use engine::{Engine, EngineParts, ExportReport, EXPORT_BATCH_FRAMES};
pub use registry::Registry;
