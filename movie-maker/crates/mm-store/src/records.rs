//! 操作の記録まわりの型。トランザクションの形は `mm-ops` の [`TxnRecord`]（操作 1 つは [`OpRecord`]）で、
//! 保存の層はそれに「誰が、いつ、どのコメントへの対応か」を足して表に書き込む。

use serde::{Deserialize, Serialize};

pub use mm_ops::{OpRecord, TxnKind, TxnRecord};

/// 誰が。操作経路の種類（`gui`、`cli`、`claude`、`mcp`）と名前。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: String,
    pub name: String,
}

/// 書き込むトランザクション（ドラッグ、マクロ、Claude の対応 1 回、取り消し、やり直し）。
/// ID とバージョンは `Store` が振る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TxnInput {
    /// 普通の操作か、どのトランザクションの取り消し・やり直しか。
    pub kind: TxnKind,
    /// 表示用の名前（「ドラッグ」「Claude の対応」）。
    pub name: String,
    pub actor: Actor,
    /// 時刻（UNIX のミリ秒）。
    pub at_ms: i64,
    /// どのコメントへの対応か。
    pub responds_to: Option<String>,
    /// 適用した順の操作。空は拒否する。
    pub ops: Vec<OpRecord>,
}

impl TxnInput {
    /// 普通の操作のトランザクション。
    pub fn normal(name: impl Into<String>, actor: Actor, at_ms: i64, ops: Vec<OpRecord>) -> Self {
        Self {
            kind: TxnKind::Normal,
            name: name.into(),
            actor,
            at_ms,
            responds_to: None,
            ops,
        }
    }
}

/// 表から読んだ操作。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredOp {
    pub seq: i64,
    /// 適用した後のバージョン。
    pub doc_version: i64,
    pub name: String,
    pub record: OpRecord,
}

/// 表から読んだトランザクション（`mm diff`、`mm log`、取り消しの元）。[`TxnRecord`] に誰が・いつ・何への対応かを足した形。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTxn {
    /// トランザクションの通し番号（1 から）。
    pub id: i64,
    pub kind: TxnKind,
    pub name: String,
    pub actor: Actor,
    pub at_ms: i64,
    pub responds_to: Option<String>,
    /// 適用する前のバージョン。
    pub base_version: i64,
    /// 適用した後のバージョン。取り消されても変わらない。
    pub doc_version: i64,
    pub first_seq: i64,
    pub last_seq: i64,
    pub ops: Vec<StoredOp>,
}

impl StoredTxn {
    /// `mm-ops` の形。
    pub fn record(&self) -> TxnRecord {
        TxnRecord {
            id: self.id as u64,
            kind: self.kind,
            name: self.name.clone(),
            base_version: self.base_version as u64,
            version: self.doc_version as u64,
            ops: self.ops.iter().map(|o| o.record.clone()).collect(),
        }
    }
}

/// 取り消しかやり直しの計画。`ops` は適用した順で、取り消しは逆順に `inverse` を適用する。
#[derive(Debug, Clone, PartialEq)]
pub struct UndoPlan {
    pub txn_id: i64,
    pub name: String,
    pub ops: Vec<OpRecord>,
}

/// `versions` の 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub name: String,
    pub at_ms: i64,
    pub actor: String,
    pub doc_version: i64,
    pub ops_seq: i64,
    /// 書き出した JSON のハッシュ（16 進）。
    pub hash: String,
    pub git_commit: Option<String>,
}

/// `txns.kind` の文字。
pub(crate) fn kind_str(kind: TxnKind) -> &'static str {
    match kind {
        TxnKind::Normal => "normal",
        TxnKind::Undo { .. } => "undo",
        TxnKind::Redo { .. } => "redo",
    }
}

/// `txns.kind` と `of_txn` から。知らない文字は普通の操作として読む。
pub(crate) fn kind_of(kind: &str, of: Option<i64>) -> TxnKind {
    match (kind, of) {
        ("undo", Some(of)) => TxnKind::Undo { of: of as u64 },
        ("redo", Some(of)) => TxnKind::Redo { of: of as u64 },
        _ => TxnKind::Normal,
    }
}
