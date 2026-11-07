//! sequence とトランジションを通る参照、同じ時刻のマーカー（`Project::validate` と `load`）のテスト。

use mm_doc::{
    Composition, Cut, Layer, Marker, Node, Project, Sequence, Transition, NESTED_KIND,
    NESTED_PARAM, SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_types::{Id, MmError, ParamValue, Rational, RationalTime, Result};

fn comp(id: &str, layers: Vec<Layer>) -> Composition {
    Composition::new(
        Id::new(id),
        16,
        9,
        Rational::integer(30),
        RationalTime::seconds(2),
    )
    .with_layers(layers)
}

fn layer(id: &str, node: Node) -> Layer {
    Layer::new(
        id,
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(1),
    )
}

fn project(comps: Vec<Composition>) -> Project {
    Project {
        compositions: comps,
        ..Project::new()
    }
}

fn is_invalid(r: Result<()>) -> bool {
    matches!(r, Err(MmError::InvalidArgument(_)))
}

fn nested(target: &str) -> Node {
    Node::new(NESTED_KIND, 1).with_param(NESTED_PARAM, ParamValue::Text(target.into()))
}

fn sequence_node(target: &str) -> Node {
    Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text(target.into()))
}

fn seq_with_cut_source(id: &str, source: Node) -> Sequence {
    Sequence {
        id: Id::new(id),
        cuts: vec![Cut::new(
            format!("{id}-k"),
            source,
            RationalTime::ZERO,
            RationalTime::seconds(1),
        )],
        origin: None,
    }
}

#[test]
fn どこからも参照されない_sequence_の循環も拒否する() {
    let mut p = project(vec![]);
    p.sequences
        .push(seq_with_cut_source("s", sequence_node("s")));
    assert!(is_invalid(p.validate()));
}

#[test]
fn sequence_どうしの循環を拒否する() {
    let mut p = project(vec![]);
    p.sequences
        .push(seq_with_cut_source("a", sequence_node("b")));
    p.sequences
        .push(seq_with_cut_source("b", sequence_node("a")));
    assert!(is_invalid(p.validate()));
}

#[test]
fn 循環した_sequence_を参照するレイヤーがあっても_validate_はクラッシュせずに拒否する() {
    let mut p = project(vec![comp("c", vec![layer("l", sequence_node("s"))])]);
    p.sequences
        .push(seq_with_cut_source("s", sequence_node("s")));
    assert!(is_invalid(p.validate()));
}

#[test]
fn トランジションのノードも入れ子と_sequence_の先を確かめる() {
    let mut p = project(vec![comp("c", vec![])]);
    let mut seq = seq_with_cut_source("s", Node::new("solid", 1));
    seq.cuts[0].transition_in = Some(Transition {
        node: nested("zzz"),
        duration: RationalTime::ZERO,
    });
    p.sequences.push(seq);
    assert!(is_invalid(p.validate()), "無い入れ子を拒否する");
    // c のレイヤーが s を参照し、s のトランジションが c を入れ子にする循環。
    p.compositions[0]
        .layers
        .push(layer("l", sequence_node("s")));
    p.sequences[0].cuts[0].transition_in = Some(Transition {
        node: nested("c"),
        duration: RationalTime::ZERO,
    });
    assert!(
        is_invalid(p.validate()),
        "トランジション経由の循環を拒否する"
    );
}

fn marker_at(id: &str, t: i64) -> Marker {
    Marker {
        id: Id::new(id),
        time: RationalTime::seconds(t),
        duration: RationalTime::ZERO,
        name: id.into(),
    }
}

#[test]
fn 同じ時刻のマーカーを拒否する() {
    let mut c = comp("c", vec![]);
    c.markers = vec![marker_at("m1", 1), marker_at("m2", 1)];
    assert!(is_invalid(project(vec![c]).validate()));
}

#[test]
fn 同じ時刻のマーカーを持つ古いプロジェクトのデータは_load_でずらして読める() {
    let mut c = comp("c", vec![]);
    c.markers = vec![marker_at("m1", 1), marker_at("m2", 1), marker_at("m3", 1)];
    let mut old = project(vec![c]);
    old.schema_version = 3;
    let json = serde_json::to_string(&old).unwrap();
    let loaded = mm_doc::load(&json).unwrap();
    let times: Vec<_> = loaded.compositions[0]
        .markers
        .iter()
        .map(|m| m.time)
        .collect();
    assert_eq!(times[0], RationalTime::seconds(1));
    assert!(times[0] < times[1] && times[1] < times[2]);
    // ずらしは 1 フレーム（1/30 秒）より小さい。
    let frame = RationalTime::new(1, 30).unwrap();
    assert!(times[2] < RationalTime::seconds(1).checked_add(frame).unwrap());
}
