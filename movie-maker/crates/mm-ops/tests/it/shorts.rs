//! 本編とショートの操作。参照のショートは本編のカットに従い、上書きした in/out だけは従わない。

use crate::common;

use common::{canon, project, quarters, secs};
use mm_doc::{OriginRelation, Project, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_ops::*;
use mm_types::{Id, ParamValue};

fn apply(p: &mut Project, op: impl Into<Op>) -> Applied {
    op.into().apply(p, &OpContext::human()).expect("適用できる")
}

fn op_json(json: &str) -> Op {
    serde_json::from_str(json).expect("操作の JSON が読める")
}

/// 本編 s1（k1〜k4）の k2〜k3 を参照するショート short（r0, r1）。
fn with_reference() -> Project {
    let mut p = project();
    apply(
        &mut p,
        op_json(
            r#"{"op": "forkSequence", "sequence": "s1", "new_sequence": "short",
                "relation": "reference", "cuts": ["r0", "r1"],
                "range": {"first": "k2", "last": "k3"}}"#,
        ),
    );
    p
}

fn cut(p: &Project, id: &str) -> mm_doc::Cut {
    p.cut(&Id::new(id)).unwrap().1.clone()
}

#[test]
fn 範囲の参照は続いたカットだけを元つきでコピーする() {
    let p = with_reference();
    let short = p.sequence(&Id::new("short")).unwrap();
    assert_eq!(short.cuts.len(), 2);
    assert_eq!(short.cuts[0].origin.as_ref().unwrap().as_str(), "k2");
    assert_eq!(short.cuts[1].origin.as_ref().unwrap().as_str(), "k3");
    let k2 = p.cut(&Id::new("k2")).unwrap().1;
    assert_eq!(
        short.cuts[0].origin_base,
        Some(mm_doc::CutBase::of(k2)),
        "参照も比べ先を持つ"
    );
}

#[test]
fn 本編をリップルすると参照のショートも変わり_逆操作で戻る() {
    let mut p = with_reference();
    let before = p.clone();
    let applied = apply(
        &mut p,
        Ripple {
            cut: Id::new("k2"),
            edge: Edge::Out,
            by: secs(-1),
        },
    );
    assert_eq!(cut(&p, "r0").out_point, secs(4));
    assert!(
        applied.touches.iter().any(|t| t == "r0"),
        "ショートのカットにも触る"
    );
    apply(&mut p, applied.inverse);
    assert_eq!(canon(&p), canon(&before));
}

#[test]
fn 上書きした_out_は本編のリップルで変わらず_上書きを外すと従う() {
    let mut p = with_reference();
    let set = apply(
        &mut p,
        op_json(r#"{"op": "overrideCut", "cut": "r0", "out": {"num": 9, "den": 2}}"#),
    );
    assert!(cut(&p, "r0").overrides.out_point);
    assert!(set.touches.iter().any(|t| t == "k2"), "元のカットを読む");
    apply(
        &mut p,
        Ripple {
            cut: Id::new("k2"),
            edge: Edge::Out,
            by: secs(-1),
        },
    );
    let r0 = cut(&p, "r0");
    assert_eq!(r0.out_point, quarters(18), "上書きした out は残る");
    apply(&mut p, op_json(r#"{"op": "resetCut", "cut": "r0"}"#));
    let r0 = cut(&p, "r0");
    assert!(!r0.overrides.any());
    assert_eq!(r0.out_point, secs(4), "外すと今の本編に従う");
}

#[test]
fn 参照のカットを直接リップルすると拒否する() {
    let mut p = with_reference();
    let err = Op::from(Ripple {
        cut: Id::new("r0"),
        edge: Edge::Out,
        by: secs(-1),
    })
    .apply(&mut p, &OpContext::human());
    assert!(err.is_err());
    // 上書きした out はショートの中でリップルできる。
    apply(
        &mut p,
        OverrideCut {
            cut: Id::new("r0"),
            in_point: None,
            out_point: Some(secs(5)),
        },
    );
    apply(
        &mut p,
        Ripple {
            cut: Id::new("r0"),
            edge: Edge::Out,
            by: secs(-1),
        },
    );
    assert_eq!(cut(&p, "r0").out_point, secs(4));
}

#[test]
fn 上書きの逆操作は前の上書きに戻す() {
    let mut p = with_reference();
    apply(
        &mut p,
        OverrideCut {
            cut: Id::new("r0"),
            in_point: Some(quarters(9)),
            out_point: None,
        },
    );
    let before = p.clone();
    let applied = apply(
        &mut p,
        OverrideCut {
            cut: Id::new("r0"),
            in_point: None,
            out_point: Some(quarters(18)),
        },
    );
    apply(&mut p, applied.inverse);
    assert_eq!(canon(&p), canon(&before));
}

#[test]
fn 本編でないカットの上書きは拒否する() {
    let mut p = with_reference();
    let err = Op::from(OverrideCut {
        cut: Id::new("k1"),
        in_point: Some(secs(1)),
        out_point: None,
    })
    .apply(&mut p, &OpContext::human());
    assert!(err.is_err());
}

#[test]
fn 複製は元の値を残し_元が変わったカットを知らせる() {
    let mut p = project();
    apply(
        &mut p,
        ForkSequence {
            sequence: Id::new("s1"),
            new_sequence: Id::new("fork"),
            relation: OriginRelation::Fork,
            cuts: (0..4).map(|i| Id::new(format!("f{i}"))).collect(),
            range: None,
        },
    );
    assert!(p.origin_changes(&Id::new("fork")).unwrap().is_empty());
    apply(
        &mut p,
        Ripple {
            cut: Id::new("k3"),
            edge: Edge::In,
            by: quarters(1),
        },
    );
    assert_eq!(cut(&p, "f2").in_point, secs(1), "複製は従わない");
    let changes = p.origin_changes(&Id::new("fork")).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].cut.as_str(), "f2");
    assert_eq!(changes[0].fields, vec!["in"]);
}

#[test]
fn 再フレーミングと字幕の置き場所はレイヤーごとに置けてキーフレームも打てる() {
    let mut p = with_reference();
    let node = mm_doc::Node::new(SEQUENCE_KIND, 1)
        .with_param(SEQUENCE_PARAM, ParamValue::Text("short".into()));
    let layer = mm_doc::Layer::new("vl", node, secs(0), secs(0), secs(3));
    apply(
        &mut p,
        AddLayer {
            composition: Id::new("c2"),
            layer,
            index: None,
        },
    );
    apply(
        &mut p,
        op_json(
            r#"{"op": "setReframe", "layer": "vl", "cut": "r0", "transform": {
                "position": {"value": {"type": "vec2", "value": [40.0, 0.0]}},
                "scale": {"value": {"type": "number", "value": 3.0}},
                "rotation": {"value": {"type": "number", "value": 0.0}},
                "anchor": null}}"#,
        ),
    );
    let key = SetKeyframe {
        target: ParamRef::layer(&Id::new("vl"), "reframe.r0.position"),
        keyframe: mm_doc::Keyframe::new(
            secs(3),
            ParamValue::Vec2([-40.0, 0.0]),
            mm_doc::Interpolation::Linear,
        ),
    };
    apply(&mut p, key);
    apply(
        &mut p,
        op_json(
            r#"{"op": "setCaption", "layer": "vl", "caption": {
                "position": {"value": {"type": "vec2", "value": [90.0, 280.0]}},
                "width": {"value": {"type": "number", "value": 160.0}}}}"#,
        ),
    );
    let (_, l) = p.layer(&Id::new("vl")).unwrap();
    let reframe = l.framing.reframe(&Id::new("r0")).unwrap();
    assert_eq!(reframe.transform.position.keyframes.len(), 1);
    assert!(l.framing.caption.is_some());
    // 別の sequence のカットには置けない。
    let err = Op::from(SetReframe {
        layer: Id::new("vl"),
        cut: Id::new("k1"),
        transform: Some(mm_doc::Transform::default()),
    })
    .apply(&mut p, &OpContext::human());
    assert!(err.is_err());
}
