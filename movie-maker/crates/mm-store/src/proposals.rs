//! Claude の編集の提案。トランザクションを適用せずに `proposals` の表へ保留し、人が承認すると
//! [`Store::apply_txn`] と同じ道で適用し、却下するとプロジェクトのデータは変えない。
//!
//! 承認で適用したトランザクションは普通のトランザクションなので、取り消しとやり直しがそのまま効く。
//! 提案の行は、承認・却下したときと、却下を保留に戻したときだけ書き換える。

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_ops::{
    preview_ops, select_ops, Entry, Op, OpContext, OpError, OpResult, Touches, Transaction,
    TxnOutcome,
};
use mm_types::{MmError, Result};

use crate::records::{Actor, StoredTxn};
use crate::store::Store;
use crate::SqlExt;

/// 提案の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    /// 保留中。プロジェクトのデータに入っていない。
    Pending,
    /// 承認して適用した（取り消されたかは [`StoredProposal::applied`]）。
    Approved,
    /// 却下した。プロジェクトのデータは変えていない。
    Rejected,
}

impl ProposalState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    /// 表の値から戻す。知らない値を保留と読むと承認できてしまうので、壊れた行として拒否する。
    fn parse(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            other => Err(MmError::Corrupt(format!("提案の状態が読めない: {other}"))),
        }
    }
}

/// 表から読んだ提案。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredProposal {
    /// 通し番号（1 から）。
    pub id: i64,
    pub name: String,
    /// 提案した操作経路。承認で適用するときもこの経路として扱い、ロックの付いた所は拒否する。
    pub entry: Entry,
    /// 提案した者。承認で適用したトランザクションの発行元にもなる。
    pub actor: Actor,
    pub at_ms: i64,
    pub responds_to: Option<String>,
    /// 提案を作ったときのプロジェクトのデータのバージョン。
    pub base_version: i64,
    pub ops: Vec<Op>,
    /// 受け付けたときに適用して求めた、触る所。
    pub touches: Vec<String>,
    pub state: ProposalState,
    pub decided_by: Option<Actor>,
    pub decided_at_ms: Option<i64>,
    /// 承認で適用したトランザクション。
    pub txn_id: Option<i64>,
    /// 承認した操作の番号（昇順）。
    pub approved_ops: Option<Vec<usize>>,
    /// 承認で適用したトランザクションが、今も取り消されていない。
    pub applied: bool,
    /// 承認で適用したトランザクションが取り消しのスタックの一番上（次の取り消しで戻る）。
    pub undoable: bool,
}

/// 提案を書くときの、トランザクションの外の情報。
struct Meta<'a> {
    entry: Entry,
    actor: &'a Actor,
    at_ms: i64,
    responds_to: Option<String>,
    base: i64,
}

const SELECT: &str =
    "SELECT p.id, p.name, p.entry, p.actor_kind, p.actor_name, p.at_ms, p.responds_to,
       p.base_version, p.ops, p.touches, p.state, p.decided_kind, p.decided_name, p.decided_at_ms,
       p.txn_id, p.approved_ops,
       EXISTS (SELECT 1 FROM undo_stack u WHERE u.txn_id = p.txn_id AND u.pos <= ?1),
       EXISTS (SELECT 1 FROM undo_stack u WHERE u.txn_id = p.txn_id AND u.pos = ?1)
FROM proposals p";

impl Store {
    /// トランザクションを提案として保留する。今のプロジェクトのデータのコピーに適用して検証し（ロック、検証、文字起こし）、
    /// 触る所を記録する。`base_version` が古く、その後の操作と触る所が重なれば [`OpError::Conflict`]。
    pub fn propose(
        &mut self,
        txn: Transaction,
        ctx: &OpContext,
        actor: Actor,
        at_ms: i64,
        responds_to: Option<String>,
    ) -> OpResult<StoredProposal> {
        self.require_writer()?;
        let current = self.doc_version();
        let base = txn.base_version.map_or(current, |b| b as i64);
        if base > current {
            return Err(OpError::FutureBase {
                base: base as u64,
                current: current as u64,
            });
        }
        if txn.ops.is_empty() {
            return Err(MmError::InvalidArgument("操作の無い提案".into()).into());
        }
        let (after, outcome) = preview_ops(self.project(), &txn.ops, ctx)?;
        self.check_edited_words(&after)?;
        let conflict = self.touches_after(base)?.intersection(&outcome.touches);
        if !conflict.is_empty() {
            return Err(OpError::Conflict {
                base: base as u64,
                touches: conflict,
            });
        }
        let meta = Meta {
            entry: ctx.entry,
            actor: &actor,
            at_ms,
            responds_to,
            base,
        };
        let id = self.insert_proposal(&txn, &meta, &outcome.touches)?;
        Ok(self.proposal(id)?)
    }

    fn insert_proposal(&self, txn: &Transaction, meta: &Meta, touches: &Touches) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO proposals(name, entry, actor_kind, actor_name, at_ms, responds_to,
                                       base_version, ops, touches, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending')",
                params![
                    txn.name,
                    entry_str(meta.entry)?,
                    meta.actor.kind,
                    meta.actor.name,
                    meta.at_ms,
                    meta.responds_to,
                    meta.base,
                    serde_json::to_string(&txn.ops)?,
                    serde_json::to_string(&touches.to_vec())?,
                ],
            )
            .sql()?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 全部の提案（新しい順）。
    pub fn proposals(&self) -> Result<Vec<StoredProposal>> {
        let sql = format!("{SELECT} ORDER BY p.id DESC");
        let mut st = self.conn.prepare_cached(&sql).sql()?;
        let rows = st
            .query_map([self.undo_depth()], read_row)
            .sql()?
            .collect::<rusqlite::Result<Vec<_>>>()
            .sql()?;
        rows.into_iter().map(RawProposal::into_stored).collect()
    }

    /// 提案 1 つ。無ければ `NotFound`。
    pub fn proposal(&self, id: i64) -> Result<StoredProposal> {
        let sql = format!("{SELECT} WHERE p.id = ?2");
        self.conn
            .query_row(&sql, params![self.undo_depth(), id], read_row)
            .optional()
            .sql()?
            .ok_or_else(|| MmError::NotFound(format!("提案 {id}")))?
            .into_stored()
    }

    /// 保留中の提案のうち、提案の後に適用された操作と触る所が重なる所。空なら承認できる。
    pub fn proposal_conflict(&self, proposal: &StoredProposal) -> OpResult<Touches> {
        if proposal.state != ProposalState::Pending {
            return Ok(Touches::new());
        }
        let touches: Touches = proposal.touches.iter().cloned().collect();
        Ok(self
            .touches_after(proposal.base_version)?
            .intersection(&touches))
    }

    /// 保留中の提案（`selection` は承認する操作の番号、`None` は全部）を今のプロジェクトのデータのコピーに適用する。書かない。
    /// 承認した提案は今のデータに入っているので、もう一度適用すると操作が二重になる。保留中でなければ拒否する。
    pub fn preview_proposal(
        &self,
        id: i64,
        selection: Option<&[usize]>,
    ) -> OpResult<(Project, TxnOutcome)> {
        let proposal = self.pending(id)?;
        let ops = select_ops(&proposal.ops, selection)?;
        preview_ops(self.project(), &ops, &OpContext::new(proposal.entry))
    }

    /// 保留中の提案を承認して適用する。`selection` は承認する操作の番号（`None` は全部）。
    /// 提案の名前、発行元、対応するコメントのまま、提案を作ったバージョンを元にしたトランザクションとして
    /// [`Store::apply_txn`] と同じ判定（衝突、ロック、検証）で書き、同じ SQLite のトランザクションで提案を承認済みにする。
    pub fn approve_proposal(
        &mut self,
        id: i64,
        selection: Option<&[usize]>,
        decider: &Actor,
        at_ms: i64,
    ) -> OpResult<(StoredProposal, StoredTxn)> {
        let proposal = self.pending(id)?;
        let ops = select_ops(&proposal.ops, selection)?;
        let indices = approved_indices(proposal.ops.len(), selection);
        let txn =
            Transaction::new(proposal.name.clone(), ops).based_on(proposal.base_version as u64);
        let ctx = OpContext::new(proposal.entry);
        let (project, input) = self.prepare_txn(
            txn,
            &ctx,
            proposal.actor.clone(),
            at_ms,
            proposal.responds_to.clone(),
        )?;
        let stored = self.commit_with(&project, &input, |tx, stored| {
            let changed = tx
                .execute(
                    "UPDATE proposals SET state = 'approved', decided_kind = ?2, decided_name = ?3,
                            decided_at_ms = ?4, txn_id = ?5, approved_ops = ?6
                     WHERE id = ?1 AND state = 'pending'",
                    params![
                        id,
                        decider.kind,
                        decider.name,
                        at_ms,
                        stored.id,
                        serde_json::to_string(&indices)?
                    ],
                )
                .sql()?;
            expect_one(changed, id)
        })?;
        Ok((self.proposal(id)?, stored))
    }

    /// 保留中の提案を却下する。プロジェクトのデータは変えない。
    pub fn reject_proposal(
        &mut self,
        id: i64,
        decider: &Actor,
        at_ms: i64,
    ) -> Result<StoredProposal> {
        self.require_writer()?;
        self.pending(id)?;
        let changed = self
            .conn
            .execute(
                "UPDATE proposals SET state = 'rejected', decided_kind = ?2, decided_name = ?3,
                        decided_at_ms = ?4
                 WHERE id = ?1 AND state = 'pending'",
                params![id, decider.kind, decider.name, at_ms],
            )
            .sql()?;
        expect_one(changed, id)?;
        self.proposal(id)
    }

    /// 却下した提案を保留に戻す（却下の取り消し）。
    pub fn reopen_proposal(&mut self, id: i64) -> Result<StoredProposal> {
        self.require_writer()?;
        let proposal = self.proposal(id)?;
        if proposal.state != ProposalState::Rejected {
            return Err(MmError::InvalidArgument(format!(
                "提案 {id} は却下していない（{}）",
                proposal.state.as_str()
            )));
        }
        let changed = self
            .conn
            .execute(
                "UPDATE proposals SET state = 'pending', decided_kind = NULL, decided_name = NULL,
                        decided_at_ms = NULL
                 WHERE id = ?1 AND state = 'rejected'",
                [id],
            )
            .sql()?;
        expect_one(changed, id)?;
        self.proposal(id)
    }

    fn pending(&self, id: i64) -> Result<StoredProposal> {
        let proposal = self.proposal(id)?;
        if proposal.state != ProposalState::Pending {
            return Err(MmError::InvalidArgument(format!(
                "提案 {id} は保留中でない（{}）",
                proposal.state.as_str()
            )));
        }
        Ok(proposal)
    }
}

/// 承認した操作の番号。`select_ops` が受けた選び方を、昇順の番号の列にする。
fn approved_indices(len: usize, selection: Option<&[usize]>) -> Vec<usize> {
    match selection {
        Some(s) => {
            let mut v = s.to_vec();
            v.sort_unstable();
            v
        }
        None => (0..len).collect(),
    }
}

fn expect_one(changed: usize, id: i64) -> Result<()> {
    if changed == 1 {
        Ok(())
    } else {
        Err(MmError::InvalidArgument(format!(
            "提案 {id} の状態が読んだ後に変わった"
        )))
    }
}

fn entry_str(entry: Entry) -> Result<String> {
    match serde_json::to_value(entry)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(MmError::Internal(format!("操作経路の形が違う: {other}"))),
    }
}

/// 表の 1 行のまま。JSON の列を型に戻すのは [`RawProposal::into_stored`]（rusqlite の外のエラーにするため）。
struct RawProposal {
    id: i64,
    name: String,
    entry: String,
    actor: Actor,
    at_ms: i64,
    responds_to: Option<String>,
    base_version: i64,
    ops: String,
    touches: String,
    state: String,
    decided: Option<(String, String)>,
    decided_at_ms: Option<i64>,
    txn_id: Option<i64>,
    approved_ops: Option<String>,
    applied: bool,
    undoable: bool,
}

fn read_row(r: &Row) -> rusqlite::Result<RawProposal> {
    let decided_kind: Option<String> = r.get(11)?;
    let decided_name: Option<String> = r.get(12)?;
    Ok(RawProposal {
        id: r.get(0)?,
        name: r.get(1)?,
        entry: r.get(2)?,
        actor: Actor {
            kind: r.get(3)?,
            name: r.get(4)?,
        },
        at_ms: r.get(5)?,
        responds_to: r.get(6)?,
        base_version: r.get(7)?,
        ops: r.get(8)?,
        touches: r.get(9)?,
        state: r.get(10)?,
        decided: decided_kind.zip(decided_name),
        decided_at_ms: r.get(13)?,
        txn_id: r.get(14)?,
        approved_ops: r.get(15)?,
        applied: r.get(16)?,
        undoable: r.get(17)?,
    })
}

impl RawProposal {
    fn into_stored(self) -> Result<StoredProposal> {
        let state = ProposalState::parse(&self.state)?;
        let approved = state == ProposalState::Approved;
        Ok(StoredProposal {
            id: self.id,
            name: self.name,
            entry: serde_json::from_value(serde_json::Value::String(self.entry))?,
            actor: self.actor,
            at_ms: self.at_ms,
            responds_to: self.responds_to,
            base_version: self.base_version,
            ops: serde_json::from_str(&self.ops)?,
            touches: serde_json::from_str(&self.touches)?,
            state,
            decided_by: self.decided.map(|(kind, name)| Actor { kind, name }),
            decided_at_ms: self.decided_at_ms,
            txn_id: self.txn_id,
            approved_ops: self
                .approved_ops
                .map(|s| serde_json::from_str(&s))
                .transpose()?,
            applied: approved && self.applied,
            undoable: approved && self.undoable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ProposalState;

    #[test]
    fn 提案の状態は表の値と往復し_知らない値は保留と読まない() {
        for s in [
            ProposalState::Pending,
            ProposalState::Approved,
            ProposalState::Rejected,
        ] {
            assert_eq!(ProposalState::parse(s.as_str()).unwrap(), s);
        }
        assert!(ProposalState::parse("merged").is_err());
    }
}
