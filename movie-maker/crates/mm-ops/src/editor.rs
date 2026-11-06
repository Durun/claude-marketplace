//! プロジェクトのデータを 1 本の列で編む [`Editor`]。バージョン番号、同時書き込みの判定、取り消しとやり直し。
//!
//! メモリの上のモックの実装。保存の層（`mm_store::Store`）は同じ数え方（操作 1 つでバージョンが 1 進み、
//! 取り消しとやり直しも [`TxnRecord`] として記録する）を SQLite の上で行い、
//! 読み込みは [`Editor::with_version`] でバージョンを引き継ぐ。

use serde::{Deserialize, Serialize};

use mm_doc::Project;

use crate::context::OpContext;
use crate::error::{OpError, OpResult};
use crate::op::Op;
use crate::touches::Touches;
use crate::txn::{apply_transaction, OpRecord, Transaction};

/// トランザクションの種類。取り消しとやり直しも記録に残し、`mm diff` が集約する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TxnKind {
    /// 普通の操作のトランザクション。
    Normal,
    /// `of` のトランザクションの取り消し（逆操作を逆の順に）。
    Undo { of: u64 },
    /// `of` のトランザクションのやり直し（元の操作を適用し直す）。
    Redo { of: u64 },
}

impl TxnKind {
    /// 記録に残す名前。取り消しとやり直しは、元のトランザクションの名前に接頭辞を付ける。
    pub fn record_name(self, original: &str) -> String {
        match self {
            TxnKind::Normal => original.to_string(),
            TxnKind::Undo { .. } => format!("取り消し: {original}"),
            TxnKind::Redo { .. } => format!("やり直し: {original}"),
        }
    }

    /// 取り消しかやり直しの元のトランザクションの ID。
    pub fn of(self) -> Option<u64> {
        match self {
            TxnKind::Normal => None,
            TxnKind::Undo { of } | TxnKind::Redo { of } => Some(of),
        }
    }
}

/// 適用したトランザクションの記録。保存の層が `txns` の 1 行と `ops` の複数行に書き込む。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TxnRecord {
    /// トランザクションの通し番号（1 から）。
    pub id: u64,
    pub kind: TxnKind,
    pub name: String,
    /// 適用する前のプロジェクトのデータのバージョン。
    pub base_version: u64,
    /// 適用した後のプロジェクトのデータのバージョン。`base_version + ops.len()`（操作 1 つで 1 進む）。
    pub version: u64,
    /// 操作ごとの記録（適用した順）。
    pub ops: Vec<OpRecord>,
}

impl TxnRecord {
    /// `i` 番目の操作を適用した後のバージョン。
    pub fn op_version(&self, i: usize) -> u64 {
        self.base_version + i as u64 + 1
    }
}

/// プロジェクトのデータと、その上に適用した操作の記録。
#[derive(Debug, Clone)]
pub struct Editor {
    project: Project,
    version: u64,
    log: Vec<TxnRecord>,
    /// 操作ごとの（適用した後のバージョン、触った所）。古いバージョンを元にした操作の判定に使う。
    history: Vec<(u64, Touches)>,
    /// 取り消せる普通のトランザクションの ID の列と、今の位置。
    undo_stack: Vec<u64>,
    undo_pos: usize,
}

impl Editor {
    /// バージョン 0 のプロジェクトのデータから。
    pub fn new(project: Project) -> Self {
        Self::with_version(project, 0)
    }

    /// 保存から読んだプロジェクトのデータとバージョンから。記録は空で始まり、それより前のバージョンへの判定はできない。
    pub fn with_version(project: Project, version: u64) -> Self {
        Self {
            project,
            version,
            log: Vec::new(),
            history: Vec::new(),
            undo_stack: Vec::new(),
            undo_pos: 0,
        }
    }

    /// 今のプロジェクトのデータ。
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// 今のバージョン。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 適用したトランザクションの記録（取り消しとやり直しを含む）。
    pub fn log(&self) -> &[TxnRecord] {
        &self.log
    }

    /// 取り消せるか。
    pub fn can_undo(&self) -> bool {
        self.undo_pos > 0
    }

    /// やり直せるか。
    pub fn can_redo(&self) -> bool {
        self.undo_pos < self.undo_stack.len()
    }

    /// トランザクションを適用する。古いバージョンを元にしていて、その後の操作と触る所が重なれば `Conflict`。
    pub fn apply(&mut self, txn: Transaction, ctx: &OpContext) -> OpResult<&TxnRecord> {
        let base = txn.base_version.unwrap_or(self.version);
        if base > self.version {
            return Err(OpError::FutureBase {
                base,
                current: self.version,
            });
        }
        let before = self.project.clone();
        let outcome = apply_transaction(&mut self.project, &txn.ops, ctx)?;
        let conflict = self.touches_since(base).intersection(&outcome.touches);
        if !conflict.is_empty() {
            self.project = before;
            return Err(OpError::Conflict {
                base,
                touches: conflict,
            });
        }
        self.undo_stack.truncate(self.undo_pos);
        let id = self.record(TxnKind::Normal, txn.name, outcome.records);
        self.undo_stack.push(id);
        self.undo_pos += 1;
        Ok(self.log.last().expect("record が積んだ"))
    }

    /// 直前のトランザクションを取り消す。取り消すものが無ければ `Ok(None)`。
    pub fn undo(&mut self, ctx: &OpContext) -> OpResult<Option<&TxnRecord>> {
        if !self.can_undo() {
            return Ok(None);
        }
        let target = self.txn(self.undo_stack[self.undo_pos - 1]);
        let ops: Vec<Op> = target
            .ops
            .iter()
            .rev()
            .map(OpRecord::inverse)
            .collect::<Result<_, _>>()
            .map_err(json_error)?;
        let kind = TxnKind::Undo { of: target.id };
        let name = kind.record_name(&target.name);
        let outcome = apply_transaction(&mut self.project, &ops, ctx)?;
        self.record(kind, name, outcome.records);
        self.undo_pos -= 1;
        Ok(self.log.last())
    }

    /// 取り消したトランザクションをやり直す。やり直すものが無ければ `Ok(None)`。
    pub fn redo(&mut self, ctx: &OpContext) -> OpResult<Option<&TxnRecord>> {
        if !self.can_redo() {
            return Ok(None);
        }
        let target = self.txn(self.undo_stack[self.undo_pos]);
        let ops: Vec<Op> = target
            .ops
            .iter()
            .map(OpRecord::op)
            .collect::<Result<_, _>>()
            .map_err(json_error)?;
        let kind = TxnKind::Redo { of: target.id };
        let name = kind.record_name(&target.name);
        let outcome = apply_transaction(&mut self.project, &ops, ctx)?;
        self.record(kind, name, outcome.records);
        self.undo_pos += 1;
        Ok(self.log.last())
    }

    fn txn(&self, id: u64) -> &TxnRecord {
        self.log
            .iter()
            .find(|t| t.id == id)
            .expect("undo_stack の ID は log に在る")
    }

    /// バージョン `base` より後に適用した操作が触った所。
    fn touches_since(&self, base: u64) -> Touches {
        let mut t = Touches::new();
        for (version, touches) in self.history.iter().rev() {
            if *version <= base {
                break;
            }
            t.extend(touches);
        }
        t
    }

    /// 記録を積み、バージョンを操作の数だけ進める。
    fn record(&mut self, kind: TxnKind, name: String, ops: Vec<OpRecord>) -> u64 {
        let base_version = self.version;
        for record in &ops {
            self.version += 1;
            let touches = record.touches.iter().cloned().collect();
            self.history.push((self.version, touches));
        }
        let id = self.log.len() as u64 + 1;
        self.log.push(TxnRecord {
            id,
            kind,
            name,
            base_version,
            version: self.version,
            ops,
        });
        id
    }
}

fn json_error(e: serde_json::Error) -> OpError {
    OpError::Doc(mm_types::MmError::Json(e))
}
