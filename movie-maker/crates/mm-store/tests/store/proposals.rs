//! 提案の保留、承認、一部の承認、却下、却下の取り消しと、取り消し・やり直しとの組み合わせ。

use mm_ops::{Entry, Op, OpContext, OpError, ParamRef, SetParam, Transaction};
use mm_store::{Actor, ProposalState, Store};
use mm_types::{Id, ParamValue};

use crate::common::{project_with, toggle_lock, NOW};

pub fn opacity(layer: usize, v: f64) -> Op {
    SetParam {
        target: ParamRef::layer(&Id::new(format!("l{layer}")), "opacity"),
        value: ParamValue::Number(v),
        keyframes: None,
    }
    .into()
}

pub fn claude() -> Actor {
    Actor {
        kind: "mcp".into(),
        name: "claude".into(),
    }
}

pub fn human() -> Actor {
    Actor {
        kind: "gui".into(),
        name: "app".into(),
    }
}

pub fn propose(s: &mut Store, ops: Vec<Op>, base: Option<u64>) -> mm_ops::OpResult<i64> {
    let mut txn = Transaction::new("Claude の対応", ops);
    txn.base_version = base;
    s.propose(txn, &OpContext::claude(), claude(), NOW, Some("c1".into()))
        .map(|p| p.id)
}

fn canon(s: &Store) -> serde_json::Value {
    serde_json::to_value(s.project()).unwrap()
}

#[test]
fn 保留中の提案はプロジェクトのデータを変えず_承認で適用され取り消しで戻る() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(3), NOW).unwrap();
    let before = canon(&s);
    let id = propose(&mut s, vec![opacity(0, 0.5), opacity(1, 0.25)], None).unwrap();
    assert_eq!(canon(&s), before);
    assert_eq!(s.doc_version(), 0);

    let p = s.proposal(id).unwrap();
    assert_eq!(p.state, ProposalState::Pending);
    assert_eq!(p.entry, Entry::Mcp);
    assert_eq!(p.touches, ["l0", "l1"]);

    let (preview, _) = s.preview_proposal(id, None).unwrap();
    let (p, txn) = s.approve_proposal(id, None, &human(), NOW).unwrap();
    assert_eq!(s.project(), &preview, "承認した結果はプレビューと同じ");
    assert_eq!(p.state, ProposalState::Approved);
    assert_eq!(p.txn_id, Some(txn.id));
    assert_eq!(p.approved_ops, Some(vec![0, 1]));
    assert_eq!(p.decided_by, Some(human()));
    assert!(p.applied && p.undoable);
    assert_eq!(txn.actor, claude(), "発行元は提案した者");
    assert_eq!(txn.responds_to.as_deref(), Some("c1"));

    s.undo_last(&OpContext::human(), human(), NOW)
        .unwrap()
        .unwrap();
    assert_eq!(canon(&s), before);
    let p = s.proposal(id).unwrap();
    assert!(!p.applied && !p.undoable, "取り消すと未適用に見える");
    s.redo_last(&OpContext::human(), human(), NOW)
        .unwrap()
        .unwrap();
    assert!(s.proposal(id).unwrap().applied);
}

#[test]
fn 一部だけ承認すると選んだ操作だけが入る() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(3), NOW).unwrap();
    let id = propose(
        &mut s,
        vec![opacity(0, 0.5), opacity(1, 0.25), opacity(2, 0.75)],
        None,
    )
    .unwrap();
    let (p, txn) = s
        .approve_proposal(id, Some(&[2, 0]), &human(), NOW)
        .unwrap();
    assert_eq!(p.approved_ops, Some(vec![0, 2]));
    let touched: Vec<&str> = txn
        .ops
        .iter()
        .flat_map(|o| o.record.touches.iter().map(String::as_str))
        .collect();
    assert_eq!(touched, ["l0", "l2"]);
    assert!(
        s.approve_proposal(id, None, &human(), NOW).is_err(),
        "二度は承認しない"
    );
}

#[test]
fn 承認する操作の番号が重なる_範囲外_空は拒否する() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    let id = propose(&mut s, vec![opacity(0, 0.5), opacity(1, 0.5)], None).unwrap();
    for bad in [&[0, 0][..], &[2], &[]] {
        assert!(
            s.approve_proposal(id, Some(bad), &human(), NOW).is_err(),
            "{bad:?}"
        );
    }
    assert_eq!(s.doc_version(), 0);
    assert_eq!(s.proposal(id).unwrap().state, ProposalState::Pending);
}

#[test]
fn 却下はデータを変えず_保留に戻せる() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    let id = propose(&mut s, vec![opacity(0, 0.5)], None).unwrap();
    let p = s.reject_proposal(id, &human(), NOW).unwrap();
    assert_eq!(p.state, ProposalState::Rejected);
    assert_eq!(s.doc_version(), 0);
    assert!(s.approve_proposal(id, None, &human(), NOW).is_err());
    let p = s.reopen_proposal(id).unwrap();
    assert_eq!((p.state, p.decided_by), (ProposalState::Pending, None));
    assert!(s.reopen_proposal(id).is_err(), "保留中は戻せない");
    s.approve_proposal(id, None, &human(), NOW).unwrap();
}

#[test]
fn プレビューは保留中の提案だけで_承認や却下の後は操作を二重に適用しない() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    let approved = propose(&mut s, vec![opacity(0, 0.5)], None).unwrap();
    let rejected = propose(&mut s, vec![opacity(1, 0.5)], None).unwrap();
    assert!(s.preview_proposal(approved, None).is_ok());
    s.approve_proposal(approved, None, &human(), NOW).unwrap();
    s.reject_proposal(rejected, &human(), NOW).unwrap();
    for id in [approved, rejected] {
        let err = s.preview_proposal(id, None).unwrap_err();
        assert!(err.to_string().contains("保留中でない"), "{err}");
    }
}

#[test]
fn 提案の後に同じ所を触られたら承認は衝突で拒否し_別の所なら通る() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(3), NOW).unwrap();
    let a = propose(&mut s, vec![opacity(0, 0.5)], None).unwrap();
    let b = propose(&mut s, vec![opacity(1, 0.5)], None).unwrap();
    let human_edit = Transaction::new("ドラッグ", vec![opacity(0, 0.1)]);
    s.apply_txn(human_edit, &OpContext::human(), human(), NOW, None)
        .unwrap();

    let pa = s.proposal(a).unwrap();
    assert_eq!(s.proposal_conflict(&pa).unwrap().to_vec(), ["l0"]);
    let err = s.approve_proposal(a, None, &human(), NOW).unwrap_err();
    assert!(matches!(err, OpError::Conflict { .. }), "{err:?}");
    assert_eq!(s.proposal(a).unwrap().state, ProposalState::Pending);

    assert!(s
        .proposal_conflict(&s.proposal(b).unwrap())
        .unwrap()
        .is_empty());
    s.approve_proposal(b, None, &human(), NOW).unwrap();
}

#[test]
fn ロックの付いたレイヤーへの提案は受け付けず_後からロックされたら承認で拒否する() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    let id = propose(&mut s, vec![opacity(0, 0.5)], None).unwrap();
    let lock = toggle_lock(s.project(), 0);
    s.apply_txn(
        Transaction::new("ロック", vec![lock]),
        &OpContext::human(),
        human(),
        NOW,
        None,
    )
    .unwrap();
    let err = propose(&mut s, vec![opacity(0, 0.3)], None).unwrap_err();
    assert!(matches!(err, OpError::Locked(_)), "{err:?}");
    assert!(s.approve_proposal(id, None, &human(), NOW).is_err());
}

#[test]
fn 提案は開き直しても残り_読み取りだけでも読める() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    let id = propose(&mut s, vec![opacity(0, 0.5)], None).unwrap();
    s.approve_proposal(id, None, &human(), NOW).unwrap();
    let pending = propose(&mut s, vec![opacity(1, 0.5)], None).unwrap();
    s.close(NOW).unwrap();
    let s = Store::open_read_only(tmp.path()).unwrap();
    let list = s.proposals().unwrap();
    assert_eq!(list.iter().map(|p| p.id).collect::<Vec<_>>(), [pending, id]);
    assert!(list[1].applied);
    assert_eq!(list[0].ops, vec![opacity(1, 0.5)]);
}
