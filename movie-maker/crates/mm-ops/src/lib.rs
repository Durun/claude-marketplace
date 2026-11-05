//! 命令の層。文書（[`mm_doc::Project`]）を変える唯一の道。
//!
//! - 命令は [`Op`] の 1 つの枝で、`ops/` に 1 つ 1 ファイル。引数の型（serde + JSON Schema）、
//!   検証、適用、逆命令、`touches`（触る所の ID の集合）を持つ。
//! - 複数の命令は [`Transaction`] として当て、途中で失敗したら全部戻す。
//!   [`Editor`] が文書の版番号、同時書き込みの判定（`touches` の重なり）、取り消しとやり直しを持つ。
//! - ロックの付いたレイヤーに触る命令は、Claude の入口（[`Entry::is_claude`]）からは断る。
//! - 保存の層は [`OpRecord`] と [`TxnRecord`] をそのまま表に書く。
//! - 全部の命令の JSON Schema は [`ops_schema`]（`schema/ops.json` に凍結）。

mod context;
mod editor;
mod error;
mod op;
mod ops;
mod param_ref;
mod schema;
mod touches;
mod txn;

pub use context::{Entry, OpContext};
pub use editor::{Editor, TxnKind, TxnRecord};
pub use error::{OpError, OpResult};
pub use op::{Applied, Op};
pub use ops::*;
pub use param_ref::ParamRef;
pub use schema::{ops_schema, ops_schema_json, SCHEMA_PATH};
pub use touches::Touches;
pub use txn::{apply_transaction, OpRecord, Transaction, TxnOutcome};
