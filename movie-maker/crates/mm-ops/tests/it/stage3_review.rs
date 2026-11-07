//! 段階 3 のレビューで出た、撮影素材の参照・隙間の見分け・レイヤーの端の範囲の確認。

use crate::common;

use common::{canon, project, quarters, secs};
use mm_doc::{Cut, Layer, Node, Project, Sequence};
use mm_ops::*;
use mm_types::{Id, ParamValue, Rgba};

fn apply(p: &mut Project, op: impl Into<Op>) -> OpResult<Applied> {
    op.into().apply(p, &OpContext::human())
}

fn touches(p: &Project, op: impl Into<Op>) -> Vec<String> {
    op.into()
        .touches(p)
        .unwrap()
        .iter()
        .map(str::to_owned)
        .collect()
}

fn footage_cut(id: &str) -> Cut {
    Cut::new(id, Node::footage(&Id::new("a1")), secs(0), quarters(8))
}

#[test]
fn 撮影素材のカットを触る操作は_touches_に素材を含める() {
    let p = project();
    let insert = InsertCuts {
        sequence: Id::new("s1"),
        index: 0,
        cuts: vec![footage_cut("kg")],
    };
    assert!(touches(&p, insert).contains(&"a1".to_owned()), "insertCuts");
    assert!(
        touches(&p, Lift { cut: Id::new("kf") }).contains(&"a1".to_owned()),
        "lift"
    );
    let extract = ExtractCuts {
        sequence: Id::new("s2"),
        index: 0,
        count: 1,
    };
    assert!(
        touches(&p, extract).contains(&"a1".to_owned()),
        "extractCuts"
    );
}

#[test]
fn lift_と_remove_asset_は_touches_が重なるので順の入れ替えを判定できる() {
    let p = project();
    let lift = Op::from(Lift { cut: Id::new("kf") });
    let remove = Op::from(RemoveAsset {
        asset: Id::new("a1"),
    });
    assert!(lift
        .touches(&p)
        .unwrap()
        .overlaps(&remove.touches(&p).unwrap()));
}

#[test]
fn 利用者の透明な単色のカットは隙間ではなく_lift_で隙間にできる() {
    let mut p = project();
    let transparent =
        Node::new("solid", 1).with_param("color", ParamValue::Color(Rgba::TRANSPARENT));
    assert!(!transparent.is_gap());
    p.sequences.push(Sequence {
        id: Id::new("s3"),
        cuts: vec![Cut::new("kt", transparent, secs(0), secs(2))],
        origin: None,
    });
    apply(&mut p, Lift { cut: Id::new("kt") }).unwrap();
    assert!(p.cut(&Id::new("kt")).unwrap().1.source.is_gap());
    assert!(
        apply(&mut p, Lift { cut: Id::new("kt") }).is_err(),
        "隙間は 2 回抜けない"
    );
}

fn project_with_footage_layer() -> Project {
    let mut p = project();
    // 素材 a1 は 20 秒。素材の 5 秒から 15 秒を使う帯。
    let layer = Layer::new(
        "lf",
        Node::footage(&Id::new("a1")),
        secs(0),
        secs(5),
        secs(15),
    );
    p.compositions[1].layers.push(layer);
    p.validate().unwrap();
    p
}

fn trim(edge: Edge, by: i64) -> TrimLayer {
    TrimLayer {
        layer: Id::new("lf"),
        edge,
        by: secs(by),
    }
}

#[test]
fn 撮影素材のレイヤーの端は素材の外へ動かせない() {
    let mut p = project_with_footage_layer();
    let before = canon(&p);
    assert!(
        apply(&mut p, trim(Edge::In, -6)).is_err(),
        "イン点が負になる"
    );
    assert!(
        apply(&mut p, trim(Edge::Out, 6)).is_err(),
        "アウト点が素材の長さを超える"
    );
    assert_eq!(canon(&p), before, "拒否したときは変えない");

    apply(&mut p, trim(Edge::In, -5)).unwrap();
    apply(&mut p, trim(Edge::Out, 5)).unwrap();
    let layer = p.layer(&Id::new("lf")).unwrap().1;
    assert_eq!((layer.in_point, layer.out_point), (secs(0), secs(20)));
}

#[test]
fn trim_layer_の_touches_は素材を含める() {
    let p = project_with_footage_layer();
    assert!(touches(&p, trim(Edge::Out, 1)).contains(&"a1".to_owned()));
}
