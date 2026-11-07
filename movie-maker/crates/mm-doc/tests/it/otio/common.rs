//! OTIO のテストで使うプロジェクトと、取り込みの結果をプロジェクトに足す手順。

use mm_doc::otio::Imported;
use mm_doc::{
    Asset, Composition, Cut, Layer, Marker, Node, OriginRelation, Project, Sequence,
    SequenceOrigin, Transition,
};
use mm_types::{ColorSpace, Id, ParamValue, Rational, RationalTime, Rgba};

pub fn t(n: i128, d: u32) -> RationalTime {
    RationalTime::new(n, d).unwrap()
}

fn asset(id: &str, path: &str, fps: Option<Rational>) -> Asset {
    Asset {
        id: Id::new(id),
        path: path.into(),
        content_hash: format!("hash{id}"),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(60),
        fps,
        timebase_offset: t(1, 100),
    }
}

/// 本編の sequence `main`（撮影素材、隙間、速さ、外したカット、トランジション、単色）とそれを置いたコンポジション、
/// 本編を複製したショート `short`。
pub fn sample_project() -> Project {
    let ntsc = Rational::new(30000, 1001).unwrap();
    let mut p = Project::new();
    p.assets
        .push(asset("cam", "D:/dev/日本語 の/cam.mp4", Some(ntsc)));
    p.assets.push(asset("vfr", "D:/dev/vfr.mkv", None));
    let mut c1 = Cut::new(
        "c1",
        Node::footage(&Id::new("cam")),
        t(1001, 3000),
        t(5005, 3000),
    );
    c1.speed = Rational::new(3, 2).unwrap();
    let c2 = Cut::new("c2", Node::gap(), RationalTime::ZERO, t(1, 3));
    let mut c3 = Cut::new("c3", Node::footage(&Id::new("vfr")), t(1, 7), t(10, 7));
    c3.transition_in = Some(Transition {
        node: Node::new("transition.dissolve", 1),
        duration: t(1, 2),
    });
    let mut c4 = Cut::new("c4", Node::footage(&Id::new("cam")), t(2, 1), t(3, 1));
    c4.enabled = false;
    let solid = Node::new("solid", 1).with_param("color", ParamValue::Color(Rgba::WHITE));
    let c5 = Cut::new("c5", solid, RationalTime::ZERO, t(2, 1));
    let mut c6 = Cut::new("c6", Node::footage(&Id::new("cam")), t(0, 1), t(1, 1));
    c6.transition_in = Some(Transition {
        node: Node::new("transition.custom", 3),
        duration: t(1, 4),
    });
    p.sequences.push(Sequence {
        id: Id::new("main"),
        cuts: vec![c1, c2, c3, c4, c5, c6],
        origin: None,
    });
    let mut forked = Cut::new("s1", Node::footage(&Id::new("cam")), t(0, 1), t(2, 1));
    forked.origin = Some(Id::new("c1"));
    p.sequences.push(Sequence {
        id: Id::new("short"),
        cuts: vec![forked],
        origin: Some(SequenceOrigin {
            sequence: Id::new("main"),
            relation: OriginRelation::Fork,
        }),
    });
    p.compositions.push(main_composition());
    p.normalize();
    p.validate().expect("テストのプロジェクトは正しい");
    p
}

fn main_composition() -> Composition {
    let source = Node::new("sequence", 1).with_param("sequence", ParamValue::Text("main".into()));
    let mut layer = Layer::new("l-main", source, t(2, 1), t(1, 2), t(40, 1));
    layer.speed = Rational::new(2, 1).unwrap();
    let marker = |id: &str, time: RationalTime, name: &str| Marker {
        id: Id::new(id),
        time,
        duration: RationalTime::ZERO,
        name: name.into(),
    };
    Composition::new(
        Id::new("main-comp"),
        1280,
        720,
        Rational::integer(60),
        RationalTime::seconds(30),
    )
    .with_layers(vec![layer])
    .with_markers(vec![
        marker("m1", t(2, 1), "導入"),
        marker("m2", t(7, 2), "本題"),
    ])
}

/// 取り込んだ結果を、空のプロジェクトに足して不変条件を通す。
pub fn apply(imported: &Imported) -> Project {
    let mut p = Project::new();
    p.assets.extend(imported.assets.clone());
    p.sequences.extend(imported.sequences.clone());
    p.compositions.extend(imported.compositions.clone());
    p.normalize();
    p.validate()
        .expect("取り込んだプロジェクトが不変条件を満たす");
    p
}
