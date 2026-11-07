//! 性質テスト: 提案、承認（一部も）、却下、却下の取り消し、人の編集とロック、取り消しとやり直しを
//! どう組み合わせても、プロジェクトのデータが壊れず、全部取り消すと最初のデータにバイトで戻る。

use mm_ops::{OpContext, Transaction};
use mm_store::{ProposalState, Store, StoredProposal};
use proptest::prelude::*;

use crate::common::{project_with, toggle_lock, NOW};
use crate::proposals::{human, opacity, propose};

const LAYERS: usize = 3;
/// 不透明度の値は 0.0〜1.0 を 0.1 刻みで選ぶ。
const STEPS: u8 = 10;

#[derive(Debug, Clone)]
enum Action {
    Propose { ops: Vec<(usize, u8)>, stale: bool },
    Human { layer: usize, value: u8 },
    Lock { layer: usize },
    Approve { pick: usize, mask: u8 },
    Reject { pick: usize },
    Reopen { pick: usize },
    Undo,
    Redo,
}

fn action() -> impl Strategy<Value = Action> {
    let op = (0..LAYERS, 0..=STEPS);
    prop_oneof![
        3 => (prop::collection::vec(op.clone(), 1..4), any::<bool>())
            .prop_map(|(ops, stale)| Action::Propose { ops, stale }),
        2 => op.prop_map(|(layer, value)| Action::Human { layer, value }),
        1 => (0..LAYERS).prop_map(|layer| Action::Lock { layer }),
        3 => (any::<usize>(), any::<u8>()).prop_map(|(pick, mask)| Action::Approve { pick, mask }),
        1 => any::<usize>().prop_map(|pick| Action::Reject { pick }),
        1 => any::<usize>().prop_map(|pick| Action::Reopen { pick }),
        2 => Just(Action::Undo),
        1 => Just(Action::Redo),
    ]
}

/// 比べる状態: プロジェクトのデータ、バージョン、提案の全部。
type Snapshot = (serde_json::Value, i64, Vec<StoredProposal>);

fn snapshot(s: &Store) -> Snapshot {
    (
        serde_json::to_value(s.project()).unwrap(),
        s.doc_version(),
        s.proposals().unwrap(),
    )
}

fn picked(s: &Store, pick: usize) -> Option<StoredProposal> {
    let list = s.proposals().unwrap();
    (!list.is_empty()).then(|| list[pick % list.len()].clone())
}

/// `mask` のビットで選ぶ操作の番号。全部なら `None`（全部を承認）。
fn selection(len: usize, mask: u8) -> Option<Vec<usize>> {
    let chosen: Vec<usize> = (0..len).filter(|i| mask & (1 << i) != 0).collect();
    (chosen.len() != len).then_some(chosen)
}

/// 1 つの操作を行い、成功したかを返す。プロジェクトのデータを変えない操作は、ここで変わらないことも確かめる。
fn run(s: &mut Store, action: &Action) -> bool {
    let before = snapshot(s);
    let ok = match action {
        Action::Propose { ops, stale } => {
            let ops = ops
                .iter()
                .map(|&(l, v)| opacity(l, f64::from(v) / f64::from(STEPS)))
                .collect();
            let base = stale.then(|| s.doc_version().saturating_sub(1) as u64);
            let ok = propose(s, ops, base).is_ok();
            assert_eq!(snapshot(s).0, before.0, "提案はデータを変えない");
            ok
        }
        Action::Human { layer, value } => {
            let txn = Transaction::new("ドラッグ", vec![opacity(*layer, f64::from(*value) / 10.0)]);
            s.apply_txn(txn, &OpContext::human(), human(), NOW, None)
                .is_ok()
        }
        Action::Lock { layer } => {
            let txn = Transaction::new("ロック", vec![toggle_lock(s.project(), *layer)]);
            s.apply_txn(txn, &OpContext::human(), human(), NOW, None)
                .is_ok()
        }
        Action::Approve { pick, mask } => approve(s, *pick, *mask),
        Action::Reject { pick } => decide(s, *pick, &before, |s, id| {
            s.reject_proposal(id, &human(), NOW).is_ok()
        }),
        Action::Reopen { pick } => decide(s, *pick, &before, |s, id| s.reopen_proposal(id).is_ok()),
        Action::Undo => s
            .undo_last(&OpContext::human(), human(), NOW)
            .is_ok_and(|t| t.is_some()),
        Action::Redo => s
            .redo_last(&OpContext::human(), human(), NOW)
            .is_ok_and(|t| t.is_some()),
    };
    if !ok {
        assert_eq!(snapshot(s), before, "失敗した {action:?} は何も変えない");
    }
    ok
}

fn approve(s: &mut Store, pick: usize, mask: u8) -> bool {
    let Some(p) = picked(s, pick) else {
        return false;
    };
    let sel = selection(p.ops.len(), mask);
    let preview = s.preview_proposal(p.id, sel.as_deref()).ok();
    match s.approve_proposal(p.id, sel.as_deref(), &human(), NOW) {
        Ok(_) => {
            assert_eq!(
                Some(s.project()),
                preview.as_ref().map(|(p, _)| p),
                "承認の結果はプレビューと同じ"
            );
            true
        }
        Err(_) => false,
    }
}

/// 却下と却下の取り消しは、提案の状態だけを変え、プロジェクトのデータとバージョンを変えない。
fn decide(
    s: &mut Store,
    pick: usize,
    before: &Snapshot,
    f: impl FnOnce(&mut Store, i64) -> bool,
) -> bool {
    let Some(p) = picked(s, pick) else {
        return false;
    };
    let ok = f(s, p.id);
    let after = snapshot(s);
    assert_eq!(
        (&after.0, after.1),
        (&before.0, before.1),
        "決定はデータを変えない"
    );
    ok
}

fn check_invariants(s: &Store) {
    s.project().validate().unwrap();
    for p in s.proposals().unwrap() {
        let approved = p.state == ProposalState::Approved;
        assert_eq!(approved, p.txn_id.is_some(), "{p:?}");
        assert_eq!(approved, p.approved_ops.is_some(), "{p:?}");
        assert_eq!(
            p.state == ProposalState::Pending,
            p.decided_by.is_none(),
            "{p:?}"
        );
        assert!(!p.undoable || p.applied, "{p:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn 提案と承認と却下と取り消しをどう組み合わせてもデータは壊れない(actions in prop::collection::vec(action(), 1..16)) {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::create(tmp.path(), &project_with(LAYERS), NOW).unwrap();
        let initial = serde_json::to_value(s.project()).unwrap();
        for a in &actions {
            run(&mut s, a);
            check_invariants(&s);
        }
        let last = snapshot(&s);
        s.close(NOW).unwrap();
        let mut s = Store::open(tmp.path(), NOW).unwrap();
        prop_assert_eq!(snapshot(&s), last, "開き直しても同じ");
        while s.undo_last(&OpContext::human(), human(), NOW).unwrap().is_some() {}
        prop_assert_eq!(serde_json::to_value(s.project()).unwrap(), initial, "全部取り消すと最初に戻る");
        for p in s.proposals().unwrap() {
            prop_assert!(!p.applied, "全部取り消すと適用中の提案は無い: {:?}", p);
        }
    }
}
