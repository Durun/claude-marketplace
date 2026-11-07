use std::path::Path;

use mm_doc::{Composition, Cut, Layer, Node, Sequence, Transition, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_types::{Id, ParamValue, Rational, RationalTime};

use super::*;

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

fn cut(id: &str, from: i64, to: i64) -> Cut {
    Cut::new(id, Node::footage(&Id::new("a1")), secs(from), secs(to))
}

fn transition(code: &str, duration: RationalTime) -> Transition {
    Transition {
        node: Node::new("transition.wgsl", 1).with_param("code", ParamValue::Text(code.into())),
        duration,
    }
}

fn project(cuts: Vec<Cut>) -> Project {
    let mut p = Project::new();
    p.sequences.push(Sequence {
        id: Id::new("s1"),
        cuts,
        origin: None,
    });
    let node =
        Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text("s1".into()));
    p.compositions.push(
        Composition::new(Id::new("main"), 320, 180, Rational::integer(30), secs(60))
            .with_layers(vec![Layer::new("ls", node, secs(0), secs(0), secs(60))]),
    );
    p
}

fn vlog() -> Pack {
    Pack::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/vlog")).unwrap()
}

#[test]
fn カットの頭の位置と_トランジションの種類と長さが出る() {
    let pack = vlog();
    let code = pack.transitions.code["whip"].clone();
    let mut second = cut("k2", 10, 13);
    second.transition_in = Some(transition(&code, RationalTime::new(7, 10).unwrap()));
    let mut third = cut("k3", 20, 22);
    third.transition_in = Some(transition("fn mm_transition() {}", secs(1)));
    let state = cuts_of(
        &project(vec![cut("k1", 0, 4), second, third]),
        &Id::new("main"),
        Some(&pack),
    )
    .unwrap();
    assert_eq!(state.sequence.as_deref(), Some("s1"));
    let starts: Vec<f64> = state.cuts.iter().map(|c| c.start).collect();
    assert_eq!(starts, [0.0, 4.0, 7.0]);
    assert_eq!(state.cuts[0].transition, None);
    let named = state.cuts[1].transition.as_ref().unwrap();
    assert_eq!(named.kind.as_deref(), Some("whip"));
    assert!((named.duration - 0.7).abs() < 1e-9);
    assert_eq!(state.cuts[2].transition.as_ref().unwrap().kind, None);
}

#[test]
fn sequence_が無ければ空() {
    let mut p = project(vec![cut("k1", 0, 4)]);
    p.compositions[0].layers.clear();
    let state = cuts_of(&p, &Id::new("main"), None).unwrap();
    assert_eq!(state.sequence, None);
    assert!(state.cuts.is_empty());
}
