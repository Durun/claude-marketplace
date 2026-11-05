//! 命令を当てて書く。`mm-ops` の [`Transaction`] を文書に当て、結果を [`Store::commit`] の
//! 形（[`TxnInput`]）に写す。入口（CLI、アプリ、MCP）はこの 3 つの口だけを呼ぶ。

use mm_doc::Project;
use mm_ops::{apply_transaction, OpContext, OpError, OpRecord, OpResult, Touches, Transaction};

use crate::records::{Actor, CommitInfo, TxnInput};
use crate::store::Store;

/// 取り消しかやり直しを書いた結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryStep {
    /// 取り消し（やり直し）たトランザクションの名前。
    pub name: String,
    /// 書いた後の文書の版番号。
    pub doc_version: i64,
}

impl Store {
    /// まとまりを今の文書に当てて書く。
    ///
    /// `txn.base_version` が古ければ、その後に当たった命令（保存した記録）と触る所が重なるとき
    /// [`OpError::Conflict`]。今の版より先なら [`OpError::FutureBase`]。`None` は今の版を元にする。
    /// 失敗したときは文書も記録も変わらない。
    pub fn apply_txn(
        &mut self,
        txn: Transaction,
        ctx: &OpContext,
        actor: Actor,
        at_ms: i64,
    ) -> OpResult<CommitInfo> {
        self.require_writer()?;
        let current = self.doc_version();
        let base = txn.base_version.map_or(current, |b| b as i64);
        if base > current {
            return Err(OpError::FutureBase {
                base: base as u64,
                current: current as u64,
            });
        }
        let mut project = self.project().clone();
        let outcome = apply_transaction(&mut project, &txn.ops, ctx)?;
        let conflict = self.touches_after(base)?.intersection(&outcome.touches);
        if !conflict.is_empty() {
            return Err(OpError::Conflict {
                base: base as u64,
                touches: conflict,
            });
        }
        let input = TxnInput {
            id: format!("{at_ms:x}-{:x}-{current}", std::process::id()),
            name: txn.name,
            actor,
            at_ms,
            responds_to: None,
            ops: outcome.records,
        };
        Ok(self.commit(&project, &input)?)
    }

    /// 直前のまとまりを取り消して書く。取り消すものが無ければ `Ok(None)`。
    pub fn undo_last(&mut self, ctx: &OpContext) -> OpResult<Option<HistoryStep>> {
        let Some(plan) = self.undo_plan()? else {
            return Ok(None);
        };
        let mut ops = Vec::with_capacity(plan.ops.len());
        for record in plan.ops.iter().rev() {
            ops.push(record.inverse().map_err(json_error)?);
        }
        let mut project = self.project().clone();
        apply_transaction(&mut project, &ops, ctx)?;
        let doc_version = self.commit_undo(&project)?;
        Ok(Some(HistoryStep {
            name: plan.name,
            doc_version,
        }))
    }

    /// 取り消したまとまりをやり直して書く。やり直すものが無ければ `Ok(None)`。
    pub fn redo_last(&mut self, ctx: &OpContext) -> OpResult<Option<HistoryStep>> {
        let Some(plan) = self.redo_plan()? else {
            return Ok(None);
        };
        let mut ops = Vec::with_capacity(plan.ops.len());
        for record in &plan.ops {
            ops.push(OpRecord::op(record).map_err(json_error)?);
        }
        let mut project: Project = self.project().clone();
        apply_transaction(&mut project, &ops, ctx)?;
        let doc_version = self.commit_redo(&project)?;
        Ok(Some(HistoryStep {
            name: plan.name,
            doc_version,
        }))
    }

    /// 版 `base` より後に当たった命令が触った所。
    fn touches_after(&self, base: i64) -> OpResult<Touches> {
        let mut touches = Touches::new();
        for (_, ids) in self.touches_since(base)? {
            for id in &ids {
                touches.add_str(id);
            }
        }
        Ok(touches)
    }
}

fn json_error(e: serde_json::Error) -> OpError {
    OpError::Doc(mm_types::MmError::Json(e))
}
