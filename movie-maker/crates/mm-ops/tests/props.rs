//! 性質テスト。
//! - 当ててから逆命令を当てると元の文書に戻る（`normalize` した JSON で比べる）。
//! - 当てた後は `Project::validate` が通る。
//! - `touches` が重ならない 2 つの命令は、順を入れ替えても同じ文書になる。
//! - トランザクションの途中で失敗すると文書は変わらない。

mod common;
mod spec;

use common::{canon, project, secs, solid};
use mm_doc::{Cut, Project};
use mm_ops::*;
use mm_types::{Id, RationalTime};
use proptest::prelude::*;
use spec::{arb_spec, materialize};

/// 見本の文書に、種で作った命令を何個か当てて散らした文書。
fn arb_project() -> impl Strategy<Value = Project> {
    prop::collection::vec(arb_spec(), 0..6).prop_map(|specs| {
        let mut p = project();
        let human = OpContext::human();
        for spec in specs {
            if let Some(op) = materialize(&spec, &p) {
                // 当たらない命令は飛ばす。文書は常に validate の通る形。
                let _ = op.apply(&mut p, &human);
            }
        }
        p
    })
}

fn outcome(p: &Project, ops: &[Op]) -> Result<serde_json::Value, String> {
    let mut p = p.clone();
    apply_transaction(&mut p, ops, &OpContext::human())
        .map(|_| canon(&p))
        .map_err(|e| e.to_string())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn 逆命令で元に戻り_当てた後は_validate_が通る(p in arb_project(), spec in arb_spec()) {
        let Some(op) = materialize(&spec, &p) else { return Ok(()) };
        let before = canon(&p);
        let mut doc = p.clone();
        let human = OpContext::human();
        let Ok(applied) = op.apply(&mut doc, &human) else {
            prop_assert_eq!(canon(&doc), before, "失敗した命令が文書を変えた: {:?}", op);
            return Ok(());
        };
        prop_assert!(doc.validate().is_ok(), "validate が通らない: {:?}", op);
        applied.inverse.apply(&mut doc, &human)
            .map_err(|e| TestCaseError::fail(format!("逆命令 {:?} が当たらない: {e}", applied.inverse)))?;
        prop_assert_eq!(canon(&doc), before, "op={:?} inverse={:?}", op, applied.inverse);
    }

    #[test]
    fn touches_が重ならない命令は順を入れ替えても同じ(p in arb_project(), a in arb_spec(), b in arb_spec()) {
        let (Some(a), Some(b)) = (materialize(&a, &p), materialize(&b, &p)) else { return Ok(()) };
        let (Ok(ta), Ok(tb)) = (a.touches(&p), b.touches(&p)) else { return Ok(()) };
        if ta.overlaps(&tb) {
            return Ok(());
        }
        let ab = outcome(&p, &[a.clone(), b.clone()]);
        let ba = outcome(&p, &[b.clone(), a.clone()]);
        prop_assert_eq!(ab.is_ok(), ba.is_ok(), "a={:?} b={:?} ab={:?} ba={:?}", a, b, ab, ba);
        if let (Ok(x), Ok(y)) = (ab, ba) {
            prop_assert_eq!(x, y, "a={:?} b={:?}", a, b);
        }
    }

    #[test]
    fn 途中で失敗したトランザクションは文書を変えない(
        p in arb_project(),
        specs in prop::collection::vec(arb_spec(), 1..5),
        at in 0usize..5,
    ) {
        let mut ops: Vec<Op> = specs.iter().filter_map(|s| materialize(s, &p)).collect();
        let failing: Op = RemoveLayer { layer: Id::new("nope") }.into();
        ops.insert(at.min(ops.len()), failing);
        let before = canon(&p);
        let mut doc = p.clone();
        prop_assert!(apply_transaction(&mut doc, &ops, &OpContext::human()).is_err());
        prop_assert_eq!(canon(&doc), before);
    }

    #[test]
    fn 命令は_json_を往復する(p in arb_project(), spec in arb_spec()) {
        let Some(op) = materialize(&spec, &p) else { return Ok(()) };
        let json = serde_json::to_value(&op).unwrap();
        prop_assert_eq!(json["op"].as_str(), Some(op.name()));
        let back: Op = serde_json::from_value(json).unwrap();
        prop_assert_eq!(back, op);
    }
}

#[test]
fn 種はどの命令も見本の文書に当たる() {
    // 各種類の命令が少なくとも 1 回は当たることを確かめ、性質テストが空回りしていないことを守る。
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let mut hit = std::collections::BTreeSet::new();
    let human = OpContext::human();
    for _ in 0..3000 {
        let spec = arb_spec().new_tree(&mut runner).unwrap().current();
        let p = project();
        let Some(op) = materialize(&spec, &p) else {
            continue;
        };
        let mut doc = p.clone();
        if op.apply(&mut doc, &human).is_ok() {
            hit.insert(op.name());
        }
    }
    let missing: Vec<_> = Op::NAMES
        .iter()
        .filter(|n| !hit.contains(*n) && **n != "unprecompose")
        .collect();
    assert!(missing.is_empty(), "当たらなかった命令: {missing:?}");
}

#[test]
fn precompose_は_unprecompose_で戻る() {
    let mut p = project();
    let human = OpContext::human();
    let before = canon(&p);
    let pre: Op = Precompose {
        composition: Id::new("c1"),
        layers: vec![Id::new("l3"), Id::new("l2")],
        new_composition: Id::new("cx"),
        new_layer: Id::new("lx"),
    }
    .into();
    // l3 の親 l1 が外に残るので断る。
    assert!(pre.apply(&mut p, &human).is_err());
    assert_eq!(canon(&p), before);
    // 続いていない範囲（l1 と l3）は断る。
    let skip: Op = Precompose {
        composition: Id::new("c1"),
        layers: vec![Id::new("l1"), Id::new("l3")],
        new_composition: Id::new("cx"),
        new_layer: Id::new("lx"),
    }
    .into();
    assert!(skip.apply(&mut p, &human).is_err());
    assert_eq!(canon(&p), before);
    let pre: Op = Precompose {
        composition: Id::new("c1"),
        layers: vec![Id::new("l3"), Id::new("l1"), Id::new("l2")],
        new_composition: Id::new("cx"),
        new_layer: Id::new("lx"),
    }
    .into();
    let applied = pre.apply(&mut p, &human).unwrap();
    let c1 = p.composition(&Id::new("c1")).unwrap();
    assert_eq!(
        c1.layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
        vec!["lx"]
    );
    let cx = p.composition(&Id::new("cx")).unwrap();
    assert_eq!(
        cx.layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
        vec!["l1", "l2", "l3"]
    );
    assert_eq!(applied.inverse.name(), "unprecompose");
    applied.inverse.apply(&mut p, &human).unwrap();
    assert_eq!(canon(&p), before);
}

#[test]
fn 見本の文書のカットは_rational_の時刻を持つ() {
    let p = project();
    let total: RationalTime = p.sequences[0]
        .cuts
        .iter()
        .map(|c: &Cut| c.duration().unwrap())
        .try_fold(RationalTime::ZERO, |a, d| a.checked_add(d))
        .unwrap();
    assert_eq!(total, secs(2 + 3 + 1 + 8));
    let _ = solid();
}
