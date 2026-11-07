//! 操作の層。プロジェクトのデータ（[`mm_doc::Project`]）を変える唯一の道。
//!
//! - 操作は [`Op`] の 1 つのバリアントで、`ops/` に 1 つ 1 ファイル。引数の型（serde + JSON Schema）、
//!   検証、適用、逆操作、`touches`（触る所の ID の集合）を持つ。
//! - 複数の操作は [`Transaction`] として適用し、途中で失敗したら全部戻す。
//!   [`Editor`] がプロジェクトのデータのバージョン番号、同時書き込みの判定（`touches` の重なり）、取り消しとやり直しを持つ。
//! - ロックの付いたレイヤーに触る操作は、Claude の操作経路（[`Entry::is_claude`]）からは拒否する。
//! - 保存の層は [`OpRecord`] と [`TxnRecord`] をそのまま表に書く。
//! - Claude の編集は提案として保留でき、承認する分を [`select_ops`] で選び、[`preview_ops`] でコピーに適用して見せる。
//! - 全部の操作の JSON Schema は [`ops_schema`]（`schema/ops.json` に凍結）。

mod context;
mod editor;
mod error;
mod op;
mod ops;
mod param_ref;
mod proposal;
mod schema;
mod shorts;
mod touches;
mod txn;

pub use context::{Entry, OpContext};
pub use editor::{Editor, TxnKind, TxnRecord};
pub use error::{OpError, OpResult};
pub use op::{Applied, Op};
pub use ops::*;
pub use param_ref::ParamRef;
pub use proposal::{
    preview_ops, proposal_scope, select_ops, LayerRef, ProposalScope, SequenceSpan,
};
pub use schema::{ops_schema, ops_schema_json, SCHEMA_PATH};
pub use touches::Touches;
pub use txn::{apply_transaction, OpRecord, Transaction, TxnOutcome};
