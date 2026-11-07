//! パック。ある作り方（vlog など）の決まりを、基盤のコードの外にデータとして持つ（docs/design/13-vlog-pack.md）。
//!
//! パックは 1 つのフォルダーで、次のファイルを持つ。
//! - `pack.json`: 名前、版の番号、形式の番号（[`FORMAT`]）。
//! - `theme.json`: 色の名前、書体、動きの長さの上限、画面の大きさ（[`Theme`]）。
//! - `styles.json`: 文字の型（字幕、ラベル、注釈、テロップ）。`text` と、地の札を描く `shader.wgsl` の組（[`TextStyleSpec`]）。
//! - `mixer.json`: ミキサーの設定（バスとダッキング）。`mm_audio_api::MixerConfig` の形。
//! - `corrections.json`: 字幕の表記の直し（誤認識の語 → 正しい表記）。
//! - `macros/*.json` と `templates/*.json`: 引数を受けて操作の列を作る手順（[`MacroDef`]）。テンプレートは
//!   空のプロジェクトに当てる雛形（コンポジション、レンダーターゲット）で、形はマクロと同じ。
//!
//! 使い方は [`Pack::load`] で読み、[`check`] で検査し、[`expand`] で 1 つのトランザクションにする。
//! 操作経路はそのトランザクションを `mm op --txn` と同じ道で当てるので、取り消しも記録も普通の操作と同じ。

mod args;
mod check;
mod expand;
mod layout;
mod macros;
mod pack;
mod sticker;
mod style;
mod theme;

pub use args::{parse_time, ArgValue};
pub use check::{check, Report};
pub use expand::expand;
pub use layout::EstimateShaper;
pub use macros::{MacroDef, MacroKind, MacroSummary, ParamDef, ParamType, Step, TextStep};
pub use pack::{Manifest, Pack, FORMAT};
pub use style::{BoxSpec, Shadow, Stroke, TextStyleSpec};
pub use theme::{Canvas, Motion, Theme};
