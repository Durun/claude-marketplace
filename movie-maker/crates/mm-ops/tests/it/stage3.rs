//! 段階 3 の操作の振る舞い。選別、話題、文字起こしの修正、レイヤーの端、隙間、話題からのカット。

use crate::common;

use common::{canon, project, quarters, secs};
use mm_doc::{Node, Project, Rating, FOOTAGE_KIND};
use mm_ops::*;
use mm_types::Id;

fn apply(p: &mut Project, op: impl Into<Op>) -> OpResult<Applied> {
    op.into().apply(p, &OpContext::human())
}

#[test]
fn 話題の列から末尾にカットを作り_1_回の逆操作で全部抜ける() {
    let mut p = project();
    let before = canon(&p);
    let mut t2 = p.bins.topics[0].clone();
    t2.id = Id::new("t2");
    t2.in_point = secs(10);
    t2.out_point = secs(12);
    apply(&mut p, AddTopic { topic: t2 }).unwrap();
    let op = InsertTopics {
        sequence: Id::new("s1"),
        index: None,
        topics: vec![Id::new("t2"), Id::new("t1")],
        cuts: vec![Id::new("x2"), Id::new("x1")],
    };
    let applied = apply(&mut p, op).unwrap();
    let cuts = &p.sequences[0].cuts;
    assert_eq!(cuts.len(), 6);
    assert_eq!(cuts[4].id.as_str(), "x2");
    assert_eq!((cuts[4].in_point, cuts[4].out_point), (secs(10), secs(12)));
    assert_eq!(cuts[5].source.kind, FOOTAGE_KIND);
    assert_eq!(applied.inverse.name(), "extractCuts");
    apply(&mut p, applied.inverse).unwrap();
    apply(
        &mut p,
        RemoveTopic {
            topic: Id::new("t2"),
        },
    )
    .unwrap();
    assert_eq!(canon(&p), before);
}

#[test]
fn 話題とカットの_id_の数が違えば拒否する() {
    let mut p = project();
    let op = InsertTopics {
        sequence: Id::new("s1"),
        index: Some(0),
        topics: vec![Id::new("t1")],
        cuts: vec![],
    };
    assert!(apply(&mut p, op).is_err());
}

#[test]
fn lift_は長さを保った隙間を残し_overwrite_で戻る() {
    let mut p = project();
    let before = canon(&p);
    let len = p.sequences[0].cuts[1].duration().unwrap();
    let applied = apply(&mut p, Lift { cut: Id::new("k2") }).unwrap();
    let k2 = &p.sequences[0].cuts[1];
    assert!(k2.source.is_gap());
    assert_eq!(k2.duration().unwrap(), len);
    assert!(apply(&mut p, Lift { cut: Id::new("k2") }).is_err());
    assert_eq!(applied.inverse.name(), "overwrite");
    apply(&mut p, applied.inverse).unwrap();
    assert_eq!(canon(&p), before);
    assert!(Node::gap().is_gap() && !common::solid().is_gap());
}

#[test]
fn レイヤーのイン点は始まりと素材の位置を一緒に動かす() {
    let mut p = project();
    // l2 は始まり 1 秒、[0, 4)。
    let op = TrimLayer {
        layer: Id::new("l2"),
        edge: Edge::In,
        by: quarters(2),
    };
    apply(&mut p, op).unwrap();
    let (_, l2) = p.layer(&Id::new("l2")).unwrap();
    assert_eq!(l2.start, quarters(6));
    assert_eq!(l2.in_point, quarters(2));
    assert_eq!(l2.range().unwrap().end, secs(5));
    let shrink_all = TrimLayer {
        layer: Id::new("l2"),
        edge: Edge::Out,
        by: secs(-4),
    };
    assert!(apply(&mut p, shrink_all).is_err());
}

#[test]
fn 印は_in_より後のアウト点だけ受け_両方外すと消える() {
    let mut p = project();
    let a = Id::new("a1");
    let mark_in = |at| MarkIn {
        asset: a.clone(),
        at,
    };
    apply(&mut p, mark_in(Some(secs(3)))).unwrap();
    let early = MarkOut {
        asset: a.clone(),
        at: Some(secs(2)),
    };
    assert!(apply(&mut p, early).is_err());
    assert_eq!(p.bins.marks[0].in_point, Some(secs(3)));
    let applied = apply(&mut p, mark_in(None)).unwrap();
    assert!(p.bins.marks.is_empty());
    apply(&mut p, applied.inverse).unwrap();
    assert_eq!(p.bins.marks[0].in_point, Some(secs(3)));
}

#[test]
fn 評価とタグは選別と話題の_id_で指し_タグは辞書順() {
    let mut p = project();
    let tag = |t: &str, remove| Tag {
        target: "sel1".into(),
        tag: t.into(),
        remove,
    };
    apply(&mut p, tag("0", false)).unwrap();
    assert_eq!(p.bins.selections[0].tags, ["0", "a"]);
    assert!(apply(&mut p, tag("a", false)).is_err());
    assert!(apply(&mut p, tag("z", true)).is_err());
    let rate = Rate {
        target: "t1".into(),
        rating: Rating::Rejected,
    };
    apply(&mut p, rate).unwrap();
    assert_eq!(p.bins.topics[0].rating, Rating::Rejected);
}

#[test]
fn 消した単語と直した文字は直しの列に残り_戻すと消える() {
    let mut p = project();
    let before = canon(&p);
    let (a, w) = (Id::new("a1"), Id::new("w1"));
    let del = DeleteWords {
        asset: a.clone(),
        words: vec![w.clone()],
        restore: false,
    };
    let undo_del = apply(&mut p, del.clone()).unwrap().inverse;
    assert!(apply(&mut p, del).is_err(), "消えた単語を消すのは拒否する");
    let fix = FixTranscript {
        asset: a.clone(),
        word: w.clone(),
        text: Some("編集".into()),
    };
    let undo_fix = apply(&mut p, fix).unwrap().inverse;
    let edit = p.word_edit(&a, &w).unwrap();
    assert!(edit.deleted && edit.text.as_deref() == Some("編集"));
    apply(&mut p, undo_fix).unwrap();
    apply(&mut p, undo_del).unwrap();
    assert_eq!(canon(&p), before);
}

#[test]
fn 選別の素材は外せず_範囲は素材の中() {
    let mut p = project();
    assert!(apply(
        &mut p,
        RemoveAsset {
            asset: Id::new("a1")
        }
    )
    .is_err());
    let mut s = p.bins.selections[0].clone();
    s.id = Id::new("sel2");
    s.out_point = secs(21);
    assert!(apply(&mut p, AddSelection { selection: s }).is_err());
    let mut dup = p.bins.selections[0].clone();
    dup.id = Id::new("t1");
    assert!(
        apply(&mut p, AddSelection { selection: dup }).is_err(),
        "話題と同じ ID の選別は拒否する"
    );
}

/// コンポジション c2 に、sequence s1 を置いた 1 秒のレイヤー ls を足す。
fn with_sequence_layer(p: &mut Project, comp_duration: mm_types::RationalTime) {
    let source = Node::new(mm_doc::SEQUENCE_KIND, 1).with_param(
        mm_doc::SEQUENCE_PARAM,
        mm_types::ParamValue::Text("s1".into()),
    );
    let mut layer = common::layer("ls", 0, 0, 1);
    layer.source = source;
    p.compositions[1].layers.push(layer);
    p.compositions[1].duration = comp_duration;
}

fn fit(layer: &str) -> FitLayer {
    FitLayer {
        layer: Id::new(layer),
        out_point: None,
        duration: None,
    }
}

#[test]
fn fit_layer_はレイヤーとコンポジションを_sequence_の長さに合わせ_逆操作で戻る() {
    let mut p = project();
    with_sequence_layer(&mut p, secs(1));
    let before = canon(&p);
    let length = p.sequences[0]
        .cuts
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.duration().unwrap())
        .fold(mm_types::RationalTime::ZERO, |a, b| {
            a.checked_add(b).unwrap()
        });
    let applied = apply(&mut p, fit("ls")).unwrap();
    let (comp, layer) = p.layer(&Id::new("ls")).unwrap();
    assert_eq!(layer.out_point, length);
    assert_eq!(comp.duration, length);
    apply(&mut p, applied.inverse).unwrap();
    assert_eq!(canon(&p), before);
}

#[test]
fn fit_layer_は自分で決めた長いコンポジションを縮めない() {
    let mut p = project();
    with_sequence_layer(&mut p, secs(100));
    apply(&mut p, fit("ls")).unwrap();
    assert_eq!(p.compositions[1].duration, secs(100));
}

fn extract_op() -> Op {
    Extract { cut: Id::new("k2") }.into()
}

#[test]
fn 長さに合わせているレイヤーは_sequence_の編集と同じトランザクションで追従し_1_回の逆で戻る() {
    let mut p = project();
    with_sequence_layer(&mut p, secs(1));
    apply(&mut p, fit("ls")).unwrap();
    let before = canon(&p);
    let ops = with_fitted_layers(&p, &[extract_op()]);
    assert_eq!(ops.len(), 2);
    let out = apply_transaction(&mut p, &ops, &OpContext::human()).unwrap();
    let (comp, layer) = p.layer(&Id::new("ls")).unwrap();
    assert_eq!(layer.duration().unwrap(), comp.duration);
    assert_eq!(
        layer.out_point,
        secs(11),
        "k2 の 3 秒が抜けて 14 秒から 11 秒"
    );
    apply_transaction(&mut p, &out.inverse_ops().unwrap(), &OpContext::human()).unwrap();
    assert_eq!(canon(&p), before);
}

#[test]
fn 自分で縮めたレイヤーには追従しない() {
    let mut p = project();
    with_sequence_layer(&mut p, secs(100));
    assert_eq!(with_fitted_layers(&p, &[extract_op()]).len(), 1);
}

#[test]
fn fit_layer_は_sequence_でないレイヤーを拒否する() {
    let mut p = project();
    assert!(apply(&mut p, fit("l1")).is_err());
}
