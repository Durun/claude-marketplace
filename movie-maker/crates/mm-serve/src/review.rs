//! 提案（Claude の編集の保留）を `Core` の直列の列で扱う。HTTP（`/proposals`）もアプリの Review パネルも
//! ここを通る。承認は普通のトランザクションとして書くので、変更の通知（`Change`）も送る。

use mm_doc::Project;
use mm_ops::{preview_ops, proposal_scope, Entry, Op, OpContext, ProposalScope, Transaction};
use mm_store::{ProposalState, Store, StoredProposal};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::core::{now_ms, Caller, Change, Core, ServeError};
use crate::frames::FrameRequest;

/// 提案の追加と決定の通知。Review パネルと `GET /events` の `proposal` が受ける。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalEvent {
    pub id: i64,
    pub state: ProposalState,
}

/// 提案の操作 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProposalOpView {
    pub index: usize,
    pub op: Op,
    /// 今のプロジェクトのデータで求めた、この操作が触る所。求められなければ空。
    pub touches: Vec<String>,
}

/// 提案を画面と HTTP に出す形。だれが、何を（操作と差分）、どの範囲に（`scope`）、状態。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProposalView {
    pub id: i64,
    pub name: String,
    /// 操作経路の種類（`mcp`、`cli`）。
    pub actor: String,
    pub actor_name: String,
    pub at_ms: i64,
    pub responds_to: Option<String>,
    pub base_version: i64,
    pub state: ProposalState,
    pub decided_by: Option<String>,
    pub decided_at_ms: Option<i64>,
    pub txn_id: Option<i64>,
    pub approved_ops: Option<Vec<usize>>,
    pub applied: bool,
    pub undoable: bool,
    /// 保留中で、提案の後の操作と触る所が重なる ID。あれば承認すると衝突で拒否する。
    pub conflict: Option<Vec<String>>,
    pub ops: Vec<ProposalOpView>,
    pub scope: ProposalScope,
}

/// 承認の結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Approval {
    pub proposal: ProposalView,
    pub change: Change,
}

fn view(store: &Store, p: StoredProposal) -> Result<ProposalView, ServeError> {
    let project = store.project();
    let conflict = store.proposal_conflict(&p)?;
    let ops = p
        .ops
        .iter()
        .enumerate()
        .map(|(index, op)| ProposalOpView {
            index,
            op: op.clone(),
            touches: op.touches(project).map(|t| t.to_vec()).unwrap_or_default(),
        })
        .collect();
    let touches = p.touches.iter().cloned().collect();
    // 決めた提案はもう適用するものではなく（承認した分は今のデータに入っている）、一覧のたびにコピーを作らない。
    // 適用できない保留中の提案（ロックや後の編集で）も、今のデータだけで範囲を引く。
    let after = (p.state == ProposalState::Pending)
        .then(|| preview_ops(project, &p.ops, &OpContext::new(p.entry)).ok())
        .flatten()
        .map(|(a, _)| a);
    let scope = proposal_scope(project, after.as_ref().unwrap_or(project), &touches);
    Ok(ProposalView {
        id: p.id,
        name: p.name,
        actor: p.actor.kind,
        actor_name: p.actor.name,
        at_ms: p.at_ms,
        responds_to: p.responds_to,
        base_version: p.base_version,
        state: p.state,
        decided_by: p.decided_by.map(|a| a.kind),
        decided_at_ms: p.decided_at_ms,
        txn_id: p.txn_id,
        approved_ops: p.approved_ops,
        applied: p.applied,
        undoable: p.undoable,
        conflict: (!conflict.is_empty()).then(|| conflict.to_vec()),
        ops,
        scope,
    })
}

/// 提案を決めるのは人だけ。提案した Claude の操作経路（MCP）が自分で承認すると、保留の意味が無くなる。
fn require_decider(caller: &Caller) -> Result<(), ServeError> {
    if caller.entry == Entry::Mcp {
        return Err(ServeError::Forbidden(
            "提案の承認・却下は MCP の操作経路からはできない".into(),
        ));
    }
    Ok(())
}

impl Core {
    /// 提案の通知を受ける。
    pub fn subscribe_proposals(&self) -> broadcast::Receiver<ProposalEvent> {
        self.proposals.subscribe()
    }

    /// 提案の通知のたびに `f` を呼ぶスレッドを始める。アプリが Review パネルへの通知に使う。
    pub fn on_proposal(
        &self,
        mut f: impl FnMut(ProposalEvent) + Send + 'static,
    ) -> std::io::Result<std::thread::JoinHandle<()>> {
        let mut rx = self.subscribe_proposals();
        std::thread::Builder::new()
            .name("serve-proposals".into())
            .spawn(move || loop {
                match rx.blocking_recv() {
                    Ok(event) => f(event),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("提案の通知を {n} 件取りこぼした");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            })
    }

    fn announce(&self, view: &ProposalView) {
        // 購読者がいなければ送り先が無いだけなので、失敗は無視する。
        let _ = self.proposals.send(ProposalEvent {
            id: view.id,
            state: view.state,
        });
    }

    /// トランザクションを適用せずに提案として保留する。
    pub fn propose(
        &self,
        txn: Transaction,
        caller: &Caller,
        responds_to: Option<String>,
    ) -> Result<ProposalView, ServeError> {
        let v = self.with_store(|s| {
            let ctx = OpContext::new(caller.entry);
            let p = s.propose(txn, &ctx, caller.actor.clone(), now_ms(), responds_to)?;
            view(s, p)
        })?;
        self.announce(&v);
        Ok(v)
    }

    /// 全部の提案（新しい順）。
    pub fn proposals(&self) -> Result<Vec<ProposalView>, ServeError> {
        self.with_store(|s| s.proposals()?.into_iter().map(|p| view(s, p)).collect())
    }

    /// 提案 1 つ。
    pub fn proposal(&self, id: i64) -> Result<ProposalView, ServeError> {
        self.with_store(|s| {
            let p = s.proposal(id)?;
            view(s, p)
        })
    }

    /// 保留中の提案を承認して適用する（`selection` は承認する操作の番号、`None` は全部）。
    pub fn approve(
        &self,
        id: i64,
        selection: Option<&[usize]>,
        caller: &Caller,
    ) -> Result<Approval, ServeError> {
        require_decider(caller)?;
        let approval = self.with_store(|s| {
            let (p, stored) = s.approve_proposal(id, selection, &caller.actor, now_ms())?;
            let change = self.finish(s, &stored, None);
            Ok(Approval {
                proposal: view(s, p)?,
                change,
            })
        })?;
        self.announce(&approval.proposal);
        Ok(approval)
    }

    /// 保留中の提案を却下する。
    pub fn reject(&self, id: i64, caller: &Caller) -> Result<ProposalView, ServeError> {
        require_decider(caller)?;
        let v = self.with_store(|s| {
            let p = s.reject_proposal(id, &caller.actor, now_ms())?;
            view(s, p)
        })?;
        self.announce(&v);
        Ok(v)
    }

    /// 却下した提案を保留に戻す。
    pub fn reopen(&self, id: i64, caller: &Caller) -> Result<ProposalView, ServeError> {
        require_decider(caller)?;
        let v = self.with_store(|s| {
            let p = s.reopen_proposal(id)?;
            view(s, p)
        })?;
        self.announce(&v);
        Ok(v)
    }

    /// 提案を適用した描画用のプロジェクトのデータ（素材のパスは絶対）と、適用した操作が触る所。書かない。
    pub fn preview_project(
        &self,
        id: i64,
        selection: Option<&[usize]>,
    ) -> Result<(Project, Vec<String>), ServeError> {
        self.with_store(|s| {
            let (mut project, outcome) = s.preview_proposal(id, selection)?;
            mm_store::rebase_project(&mut project, s.dir());
            Ok((project, outcome.touches.to_vec()))
        })
    }

    /// 提案を適用した 1 フレームの PNG。Claude と dashboard が承認の前にフレームを比べるのに使う。
    pub fn preview_frame_png(
        &self,
        id: i64,
        selection: Option<&[usize]>,
        request: &FrameRequest,
    ) -> Result<Vec<u8>, ServeError> {
        let frames = self
            .frames
            .as_ref()
            .ok_or_else(|| ServeError::Unavailable("この HTTP サーバーは描画を持たない".into()))?;
        let (project, touched) = self.preview_project(id, selection)?;
        let png = frames.frame_png(&project, request);
        // 提案を適用したフレームを、保存したデータのフレームとしてキャッシュに残さない。
        frames.invalidate(&touched);
        png.map_err(ServeError::from)
    }
}
