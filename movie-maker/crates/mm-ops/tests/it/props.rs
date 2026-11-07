//! 性質テスト。
//! - 適用してから逆操作を適用すると元のプロジェクトのデータに戻る（`normalize` した JSON で比べる）。
//! - 適用した後は `Project::validate` が通る。
//! - `touches` が重ならない 2 つの操作は、別々に 1 つずつ適用すると順を入れ替えても同じ結果になる。
//! - トランザクションの途中で失敗するとプロジェクトのデータは変わらない。
//!
//! ケース数は環境変数 `MM_PROP_CASES`（既定 400）で変える。

use crate::common;
use crate::{order, spec};

use common::{canon, project, secs, solid};
use mm_doc::{Cut, Project};
use mm_ops::*;
use mm_types::{Id, RationalTime};
use proptest::prelude::*;
use spec::{arb_spec, materialize};

fn cases() -> u32 {
    std::env::var("MM_PROP_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400)
}

/// フィクスチャのプロジェクトのデータに、種で作った操作を何個か適用して散らしたプロジェクトのデータ。
fn arb_project() -> impl Strategy<Value = Project> {
    prop::collection::vec(arb_spec(), 0..14).prop_map(|specs| {
        let mut p = project();
        let human = OpContext::human();
        for spec in specs {
            if let Some(op) = materialize(&spec, &p) {
                // 適用できない操作はスキップする。プロジェクトのデータは常に validate の通る形。
                let _ = op.apply(&mut p, &human);
            }
        }
        p
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn 逆操作で元に戻り_適用した後は_validate_が通る(p in arb_project(), spec in arb_spec()) {
        let Some(op) = materialize(&spec, &p) else { return Ok(()) };
        let before = canon(&p);
        let mut doc = p.clone();
        let human = OpContext::human();
        let Ok(applied) = op.apply(&mut doc, &human) else {
            prop_assert_eq!(canon(&doc), before, "失敗した操作がプロジェクトのデータを変えた: {:?}", op);
            return Ok(());
        };
        prop_assert!(doc.validate().is_ok(), "validate が通らない: {:?}", op);
        applied.inverse.apply(&mut doc, &human)
            .map_err(|e| TestCaseError::fail(format!("逆操作 {:?} が適用できない: {e}", applied.inverse)))?;
        prop_assert_eq!(canon(&doc), before, "op={:?} inverse={:?}", op, applied.inverse);
        // canon は素材を ID で並べ直すので、並びの戻りは別に見る。
        prop_assert_eq!(&doc.assets, &p.assets, "素材の並びが戻らない: op={:?}", op);
    }

    #[test]
    fn touches_が重ならない操作は別々に適用して順を入れ替えても同じ(
        p in arb_project(),
        specs in prop::collection::vec(arb_spec(), 2..10),
    ) {
        // 同じプロジェクトのデータに適用できる操作の全部の組を調べる。1 組ずつだと、順で変わる組を見つけにくい。
        let ops: Vec<Op> = specs.iter().filter_map(|s| materialize(s, &p)).collect();
        for (i, a) in ops.iter().enumerate() {
            for b in &ops[i + 1..] {
                if let Some(why) = order::order_violation(&p, a, b) {
                    return Err(TestCaseError::fail(why));
                }
            }
        }
    }

    #[test]
    fn 途中で失敗したトランザクションはプロジェクトのデータを変えない(
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
    fn 操作は_json_を往復する(p in arb_project(), spec in arb_spec()) {
        let Some(op) = materialize(&spec, &p) else { return Ok(()) };
        let json = serde_json::to_value(&op).unwrap();
        prop_assert_eq!(json["op"].as_str(), Some(op.name()));
        let back: Op = serde_json::from_value(json).unwrap();
        prop_assert_eq!(back, op);
    }
}

/// プロジェクトのデータが持つ参照と付属の種類。生成器が通した経路の確認に使う。
fn features(p: &Project) -> Vec<&'static str> {
    use mm_doc::{NESTED_KIND, SEQUENCE_KIND};
    let layers: Vec<_> = p
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .collect();
    let cuts: Vec<_> = p.sequences.iter().flat_map(|s| s.cuts.iter()).collect();
    let has_kind = |kind: &str| {
        let in_layers = layers.iter().flat_map(|l| l.nodes());
        let in_cuts = cuts.iter().flat_map(|c| c.nodes());
        in_layers
            .chain(in_cuts)
            .flat_map(|n| n.descendants())
            .any(|n| n.kind == kind)
    };
    let checks = [
        ("入れ子", has_kind(NESTED_KIND)),
        ("sequence のノードの種類", has_kind(SEQUENCE_KIND)),
        ("効果", layers.iter().any(|l| !l.effects.is_empty())),
        ("マスク", layers.iter().any(|l| !l.masks.is_empty())),
        ("音", layers.iter().any(|l| l.audio.is_some())),
        (
            "タイムリマップ",
            layers.iter().any(|l| l.time_remap.is_some()),
        ),
        (
            "トランジション",
            cuts.iter().any(|c| c.transition_in.is_some()),
        ),
        ("カットの origin", cuts.iter().any(|c| c.origin.is_some())),
        ("カットの上書き", cuts.iter().any(|c| c.overrides.any())),
        ("複製の元の値", cuts.iter().any(|c| c.origin_base.is_some())),
        (
            "再フレーミング",
            layers.iter().any(|l| !l.framing.cuts.is_empty()),
        ),
        ("親", layers.iter().any(|l| l.parent.is_some())),
        (
            "トラックマット",
            layers.iter().any(|l| l.track_matte.is_some()),
        ),
        ("接続", layers.iter().any(|l| l.attach_to.is_some())),
    ];
    checks
        .into_iter()
        .filter(|(_, hit)| *hit)
        .map(|(n, _)| n)
        .collect()
}

#[test]
fn 生成器は全部の操作と参照の種類を通す() {
    // 空回りしていないことを守る。どの操作も、各種の参照や付属も、少なくとも 1 回はプロジェクトのデータに出る。
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let mut hit = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut precompose_elsewhere = false;
    let human = OpContext::human();
    for _ in 0..20000 {
        let p = arb_project().new_tree(&mut runner).unwrap().current();
        seen.extend(features(&p));
        let spec = arb_spec().new_tree(&mut runner).unwrap().current();
        let Some(op) = materialize(&spec, &p) else {
            continue;
        };
        let mut doc = p.clone();
        if op.apply(&mut doc, &human).is_ok() {
            hit.insert(op.name());
            if let Op::Precompose(pre) = &op {
                precompose_elsewhere |= pre.composition != p.compositions[0].id;
            }
        }
    }
    let missing: Vec<_> = Op::NAMES.iter().filter(|n| !hit.contains(*n)).collect();
    assert!(missing.is_empty(), "適用できなかった操作: {missing:?}");
    assert!(
        precompose_elsewhere,
        "最初でないコンポジションの precompose が適用できない"
    );
    for want in [
        "入れ子",
        "sequence のノードの種類",
        "効果",
        "マスク",
        "音",
        "タイムリマップ",
        "トランジション",
        "カットの origin",
        "カットの上書き",
        "複製の元の値",
        "再フレーミング",
        "親",
        "トラックマット",
        "接続",
    ] {
        assert!(
            seen.contains(want),
            "プロジェクトのデータに出ない: {want}（出た: {seen:?}）"
        );
    }
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
    // l3 の親 l1 が外に残るので拒否する。
    assert!(pre.apply(&mut p, &human).is_err());
    assert_eq!(canon(&p), before);
    // 続いていない範囲（l1 と l3）は拒否する。
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
fn フィクスチャのプロジェクトのデータのカットは_rational_の時刻を持つ() {
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

/// 性質テストが見つけた最小の例: sequence を外す操作は、カットが参照する素材を外す操作と重なる。
#[test]
fn sequence_を外す操作は_カットが参照する素材に触る() {
    let mut p = project();
    let footage = mm_doc::Node::new("footage", 1)
        .with_param("asset", mm_types::ParamValue::AssetRef(Id::new("a1")));
    let sequence = mm_doc::Sequence {
        id: Id::new("s9"),
        cuts: vec![Cut::new("k9", footage, secs(0), secs(2))],
        origin: None,
    };
    Op::from(AddSequence { sequence })
        .apply(&mut p, &OpContext::human())
        .unwrap();
    let remove_seq = Op::from(RemoveSequence {
        sequence: Id::new("s9"),
    });
    let remove_asset = Op::from(RemoveAsset {
        asset: Id::new("a1"),
    });
    let a = remove_seq.touches(&p).unwrap();
    let b = remove_asset.touches(&p).unwrap();
    assert!(a.overlaps(&b), "a={a:?} b={b:?}");
}
