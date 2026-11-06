//! モックの実装と契約テストの本体。
//!
//! インターフェースごとにモジュールを持ち、各モジュールは `Sample*`（モックの実装）と `contract(...)`（契約テスト）を
//! 出す。実装のクレートは `tests/contract.rs` に
//! `mm_testkit::decode::contract(|| Box::new(MyFactory::new()), &fixture)` のように 1 行書く。
//!
//! モックの実装は GPU も FFmpeg も使わず、`cargo test` が数秒で回る。縦の 1 本はこのモックで先に通す。

pub mod audio;
pub mod cache;
pub mod color;
pub mod decode;
pub mod decode_vfr;
pub mod fixtures;
pub mod gpu;
pub mod host;
pub mod ingest;
pub mod node;
pub mod render;
mod render_cases;
pub mod text;
mod xform;

/// モックの実装の名前。`assemble` の設定で選ぶときの値。
pub const SAMPLE_ID: &str = "sample";
