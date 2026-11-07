//! 参照のショートの同期が本編の編集と移行で崩れないこと（v7 からの移行、上書きとの衝突、split、再フレーミングの逆操作）。

use crate::common;

use common::{canon, project, quarters, secs};
use mm_doc::{OriginRelation, Project, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_ops::*;
use mm_types::{Id, ParamValue};

fn apply(p: &mut Project, op: impl Into<Op>) -> Applied {
    op.into().apply(p, &OpContext::human()).expect("適用できる")
}

fn try_apply(p: &mut Project, op: impl Into<Op>) -> Result<Applied, String> {
    op.into()
        .apply(p, &OpContext::human())
        .map_err(|e| e.to_string())
}

fn cut(p: &Project, id: &str) -> mm_doc::Cut {
    p.cut(&Id::new(id)).unwrap().1.clone()
}

/// 本編 main（m1: 1〜3 秒、m2: 1〜3 秒）と、m1 を参照するショート short の v7 のプロジェクト。
/// ショートのカット s1 の in/out/速さは引数で決める（v7 は上書きのフラグを持たない）。
fn v7_reference(s1_in: i64, s1_out: i64, s1_speed: i64) -> Project {
    let cut = |id: &str, origin: &str, a: i64, b: i64, speed: i64| {
        format!(
            r#"{{"id": "{id}", "in": {{"num": {a}, "den": 1}}, "out": {{"num": {b}, "den": 1}},
              "speed": {{"num": {speed}, "den": 1}}, "enabled": true, "transition_in": null,
              "origin": {origin}, "source": {{"kind": "solid", "version": 1, "params": {{}}}}}}"#
        )
    };
    let json = format!(
        r#"{{"schema_version": 7, "assets": [], "compositions": [], "render_targets": [],
          "sequences": [
            {{"id": "main", "origin": null, "cuts": [{}, {}]}},
            {{"id": "short", "origin": {{"sequence": "main", "relation": "reference"}},
              "cuts": [{}]}}
          ]}}"#,
        cut("m1", "null", 1, 3, 1),
        cut("m2", "null", 1, 3, 1),
        cut("s1", r#""m1""#, s1_in, s1_out, s1_speed)
    );
    mm_doc::load(&json).expect("v7 が読める")
}

fn ripple_main_m2() -> Ripple {
    Ripple {
        cut: Id::new("m2"),
        edge: Edge::Out,
        by: secs(-1),
    }
}

#[test]
fn v7_の参照のカットの元と違う_in_out_は上書きになり_操作の後も残る() {
    let mut p = v7_reference(2, 3, 1);
    let s1 = cut(&p, "s1");
    assert!(s1.overrides.in_point && !s1.overrides.out_point);
    apply(&mut p, ripple_main_m2());
    let s1 = cut(&p, "s1");
    assert_eq!((s1.in_point, s1.out_point), (secs(2), secs(3)));
    assert_eq!(
        p.sequence(&Id::new("short"))
            .unwrap()
            .origin
            .as_ref()
            .unwrap()
            .relation,
        OriginRelation::Reference
    );
}

#[test]
fn v7_の参照のショートで速さが元と違えば複製になり_操作の後も残る() {
    let mut p = v7_reference(1, 3, 2);
    let short = p.sequence(&Id::new("short")).unwrap();
    assert_eq!(
        short.origin.as_ref().unwrap().relation,
        OriginRelation::Fork
    );
    assert!(short.cuts[0].origin_base.is_some(), "複製は比べ先を持つ");
    apply(&mut p, ripple_main_m2());
    assert_eq!(cut(&p, "s1").speed, mm_types::Rational::new(2, 1).unwrap());
}

/// 本編 s1（k1〜k4）の k2〜k3 を参照するショート short（r0, r1）。
fn with_reference() -> Project {
    let mut p = project();
    apply(
        &mut p,
        ForkSequence {
            sequence: Id::new("s1"),
            new_sequence: Id::new("short"),
            relation: OriginRelation::Reference,
            cuts: vec![Id::new("r0"), Id::new("r1")],
            range: Some(CutRange {
                first: Id::new("k2"),
                last: Id::new("k3"),
            }),
        },
    );
    p
}

#[test]
fn 本編の変更を反映するとショートの上書きした_in_以下になる操作はショートのカットと上書きを示して拒否する(
) {
    let mut p = with_reference();
    apply(
        &mut p,
        OverrideCut {
            cut: Id::new("r0"),
            in_point: Some(secs(4)),
            out_point: None,
        },
    );
    let before = p.clone();
    let err = try_apply(
        &mut p,
        Ripple {
            cut: Id::new("k2"),
            edge: Edge::Out,
            by: secs(-2),
        },
    )
    .unwrap_err();
    assert!(err.contains("k2") && err.contains("r0"), "{err}");
    assert!(err.contains("上書き"), "{err}");
    assert_eq!(canon(&p), canon(&before), "拒否したら変わらない");
}

#[test]
fn 本編のカットを割ると参照のショートの元の変化に後ろの側が出る() {
    let mut p = with_reference();
    assert!(p.origin_changes(&Id::new("short")).unwrap().is_empty());
    apply(
        &mut p,
        Split {
            cut: Id::new("k2"),
            at: secs(1),
            new_cut: Id::new("k2b"),
        },
    );
    let changes = p.origin_changes(&Id::new("short")).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].cut.as_str(), "r0");
    assert_eq!(changes[0].fields, vec!["out"]);
    assert_eq!(changes[0].split, vec![Id::new("k2b")]);
}

#[test]
fn 本編のカットの長さが変わると参照のショートの元の変化に出る() {
    let mut p = with_reference();
    apply(
        &mut p,
        Ripple {
            cut: Id::new("k3"),
            edge: Edge::In,
            by: quarters(1),
        },
    );
    let changes = p.origin_changes(&Id::new("short")).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].cut.as_str(), "r1");
    assert_eq!(changes[0].fields, vec!["in"]);
    assert!(changes[0].split.is_empty());
}

fn reframe(cut: &str, transform: Option<mm_doc::Transform>) -> SetReframe {
    SetReframe {
        layer: Id::new("vl"),
        cut: Id::new(cut),
        transform,
    }
}

#[test]
fn 再フレーミングを外す操作も検査し_逆操作が必ず通る() {
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
    apply(&mut p, reframe("r0", Some(mm_doc::Transform::default())));
    let before = p.clone();
    let removed = apply(&mut p, reframe("r0", None));
    apply(&mut p, removed.inverse);
    assert_eq!(canon(&p), canon(&before), "外した逆操作で戻る");

    apply(&mut p, Extract { cut: Id::new("r0") });
    assert!(
        try_apply(&mut p, reframe("r0", None)).is_err(),
        "消えたカットは外す操作も拒否する（逆操作が置けないため）"
    );
}
