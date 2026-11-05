//! 命令の記録まわりの型。命令 1 つの記録は `mm-ops` の [`OpRecord`] を、そのまま表に書く。

use serde::{Deserialize, Serialize};

pub use mm_ops::OpRecord;

/// 誰が。入口の種類（`gui`、`cli`、`claude`、`mcp`）と名前。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: String,
    pub name: String,
}

/// 書き込むトランザクション（ドラッグ、マクロ、Claude の対応 1 回など）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TxnInput {
    /// トランザクションの ID。呼ぶ側が一意に決める。
    pub id: String,
    /// 表示用の名前（「ドラッグ」「Claude の対応」）。
    pub name: String,
    pub actor: Actor,
    /// 時刻（UNIX のミリ秒）。
    pub at_ms: i64,
    /// どのコメントへの対応か。
    pub responds_to: Option<String>,
    /// 当てた順の命令。空は断る。
    pub ops: Vec<OpRecord>,
}

/// `commit` の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitInfo {
    pub first_seq: i64,
    pub last_seq: i64,
    /// 書いた後の文書の版番号。
    pub doc_version: i64,
}

/// トランザクションの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxnState {
    Applied,
    Undone,
}

impl TxnState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TxnState::Applied => "applied",
            TxnState::Undone => "undone",
        }
    }

    pub(crate) fn parse(s: &str) -> Self {
        if s == "undone" {
            TxnState::Undone
        } else {
            TxnState::Applied
        }
    }
}

/// 表から読んだ命令。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredOp {
    pub seq: i64,
    pub doc_version: i64,
    pub name: String,
    pub record: OpRecord,
}

/// 表から読んだトランザクション（`mm diff` と取り消しの元）。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTxn {
    pub id: String,
    pub name: String,
    pub actor: Actor,
    pub at_ms: i64,
    pub state: TxnState,
    /// 今当たっている版番号（やり直すと進む）。
    pub doc_version: i64,
    pub responds_to: Option<String>,
    pub ops: Vec<StoredOp>,
}

/// 取り消しかやり直しの計画。`ops` は当てた順で、取り消しは逆順に `inverse` を当てる。
#[derive(Debug, Clone, PartialEq)]
pub struct UndoPlan {
    pub txn_id: String,
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
