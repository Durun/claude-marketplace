//! 操作を適用して書く。`mm-ops` の [`Transaction`] をプロジェクトのデータに適用し、結果を [`Store::commit`] の
//! 形（[`TxnInput`]）に変換する。操作経路（CLI、アプリ、MCP）はこの 3 つの関数だけを呼ぶ。

use mm_ops::{
    apply_transaction, with_fitted_layers, Op, OpContext, OpError, OpRecord, OpResult, Touches,
    Transaction,
};

use crate::records::{Actor, StoredTxn, TxnInput, TxnKind, UndoPlan};
use crate::store::Store;

impl Store {
    /// トランザクションを今のプロジェクトのデータに適用して書く。
    ///
    /// `txn.base_version` が古ければ、その後に適用された操作（取り消しとやり直しを含む保存した記録）と
    /// 触る所が重なるとき [`OpError::Conflict`]。今のバージョンより先なら [`OpError::FutureBase`]。
    /// `None` は今のバージョンを元にする。失敗したときはプロジェクトのデータも記録も変わらない。
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
        // sequence の長さが変わる編集では、sequence を置いたレイヤーの長さも同じトランザクションで合わせる。
        let ops = with_fitted_layers(&project, &txn.ops);
        let outcome = apply_transaction(&mut project, &ops, ctx)?;
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

    /// 直前のトランザクションを取り消して書く。逆操作を逆順に適用し、その記録を `TxnKind::Undo` のトランザクションとして残す。
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

    /// 取り消したトランザクションをやり直して書く。元の操作を適用し直し、`TxnKind::Redo` のトランザクションとして残す。
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

    /// バージョン `base` より後に適用された操作が触った所。
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
