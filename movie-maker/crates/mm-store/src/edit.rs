//! 操作を当てて書く。`mm-ops` の [`Transaction`] を文書に当て、結果を [`Store::commit`] の
//! 形（[`TxnInput`]）に写す。入口（CLI、アプリ、MCP）はこの 3 つの口だけを呼ぶ。

use mm_ops::{apply_transaction, Op, OpContext, OpError, OpRecord, OpResult, Touches, Transaction};

use crate::records::{Actor, StoredTxn, TxnInput, TxnKind, UndoPlan};
use crate::store::Store;

impl Store {
    /// まとまりを今の文書に当てて書く。
    ///
    /// `txn.base_version` が古ければ、その後に当たった操作（取り消しとやり直しを含む保存した記録）と
    /// 触る所が重なるとき [`OpError::Conflict`]。今の版より先なら [`OpError::FutureBase`]。
    /// `None` は今の版を元にする。失敗したときは文書も記録も変わらない。
    pub fn apply_txn(
        &mut self,
        txn: Transaction,
        ctx: &OpContext,
        actor: Actor,
        at_ms: i64,
        responds_to: Option<String>,
    ) -> OpResult<StoredTxn> {
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
            kind: TxnKind::Normal,
            name: txn.name,
            actor,
            at_ms,
            responds_to,
            ops: outcome.records,
        };
        Ok(self.commit(&project, &input)?)
    }

    /// 直前のまとまりを取り消して書く。逆操作を逆順に当て、その記録を `TxnKind::Undo` のまとまりとして残す。
    /// 取り消すものが無ければ `Ok(None)`。
    pub fn undo_last(
        &mut self,
        ctx: &OpContext,
        actor: Actor,
        at_ms: i64,
    ) -> OpResult<Option<StoredTxn>> {
        let Some(plan) = self.undo_plan()? else {
            return Ok(None);
        };
        let ops = plan
            .ops
            .iter()
            .rev()
            .map(OpRecord::inverse)
            .collect::<Result<Vec<Op>, _>>()
            .map_err(json_error)?;
        let kind = TxnKind::Undo {
            of: plan.txn_id as u64,
        };
        self.commit_history(kind, &plan, ops, ctx, actor, at_ms)
            .map(Some)
    }

    /// 取り消したまとまりをやり直して書く。元の操作を当て直し、`TxnKind::Redo` のまとまりとして残す。
    /// やり直すものが無ければ `Ok(None)`。
    pub fn redo_last(
        &mut self,
        ctx: &OpContext,
        actor: Actor,
        at_ms: i64,
    ) -> OpResult<Option<StoredTxn>> {
        let Some(plan) = self.redo_plan()? else {
            return Ok(None);
        };
        let ops = plan
            .ops
            .iter()
            .map(OpRecord::op)
            .collect::<Result<Vec<Op>, _>>()
            .map_err(json_error)?;
        let kind = TxnKind::Redo {
            of: plan.txn_id as u64,
        };
        self.commit_history(kind, &plan, ops, ctx, actor, at_ms)
            .map(Some)
    }

    fn commit_history(
        &mut self,
        kind: TxnKind,
        plan: &UndoPlan,
        ops: Vec<Op>,
        ctx: &OpContext,
        actor: Actor,
        at_ms: i64,
    ) -> OpResult<StoredTxn> {
        let mut project = self.project().clone();
        let outcome = apply_transaction(&mut project, &ops, ctx)?;
        let input = TxnInput {
            kind,
            name: kind.record_name(&plan.name),
            actor,
            at_ms,
            responds_to: None,
            ops: outcome.records,
        };
        Ok(self.commit(&project, &input)?)
    }

    /// 版 `base` より後に当たった操作が触った所。
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
