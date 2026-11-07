//! テストのフィクスチャのプロジェクトのデータ。コンポジション 2 つ、レイヤー 3 つ、マーカー 2 つ、sequence 1 つ（カット 4 つ）、レンダーターゲット 1 つ。

use mm_doc::{
    Composition, Cut, Interpolation, Keyframe, Layer, Marker, Node, Project, RenderTarget, Sequence,
};
use mm_types::{Id, ParamValue, Rational, RationalTime, Rgba};

pub fn secs(s: i64) -> RationalTime {
    RationalTime::seconds(s)
}

pub fn quarters(q: i64) -> RationalTime {
    RationalTime::new(i128::from(q), 4).unwrap()
}

pub fn solid() -> Node {
    Node::new("solid", 1).with_param("color", ParamValue::Color(Rgba::WHITE))
}

pub fn layer(id: &str, start: i64, in_s: i64, out_s: i64) -> Layer {
    Layer::new(id, solid(), secs(start), secs(in_s), secs(out_s))
}

pub fn cut(id: &str, in_s: i64, out_s: i64, speed: (u32, u32)) -> Cut {
    let mut c = Cut::new(id, solid(), secs(in_s), secs(out_s));
    c.speed = Rational::new(speed.0, speed.1).unwrap();
    c
}

pub fn comp(id: &str, layers: Vec<Layer>, markers: Vec<Marker>) -> Composition {
    Composition::new(Id::new(id), 320, 180, Rational::integer(30), secs(10))
        .with_layers(layers)
        .with_markers(markers)
}

pub fn marker(id: &str, time: i64) -> Marker {
    Marker {
        id: Id::new(id),
        time: secs(time),
        duration: RationalTime::ZERO,
        name: id.to_owned(),
    }
}

pub fn project() -> Project {
    let mut l2 = layer("l2", 1, 0, 4);
    l2.opacity.keyframes = vec![
        Keyframe::new(secs(0), ParamValue::Number(0.0), Interpolation::Linear),
        Keyframe::new(secs(2), ParamValue::Number(1.0), Interpolation::Hold),
    ];
    let mut l3 = layer("l3", 2, 1, 3);
    l3.parent = Some(Id::new("l1"));
    let mut p = Project::new();
    p.compositions.push(comp(
        "c1",
        vec![layer("l1", 0, 0, 10), l2, l3],
        vec![marker("m1", 1), marker("m2", 5)],
    ));
    p.compositions.push(comp("c2", vec![], vec![]));
    p.sequences.push(Sequence {
        id: Id::new("s1"),
        cuts: vec![
            cut("k1", 0, 2, (1, 1)),
            cut("k2", 2, 5, (1, 1)),
            cut("k3", 1, 3, (2, 1)),
            cut("k4", 0, 4, (1, 2)),
        ],
        origin: None,
    });
    p.render_targets.push(RenderTarget {
        id: Id::new("r1"),
        composition: Id::new("c1"),
        width: 320,
        height: 180,
        fps: Rational::integer(30),
        codec: "sample".into(),
        output_path: "out.mp4".into(),
        loudness_lufs: None,
        delivery: Default::default(),
    });
    add_bins(&mut p);
    add_footage_sequence(&mut p);
    p.validate().unwrap();
    p
}

/// 素材 a1 の撮影素材のカット kf だけを持つ sequence s2。素材の参照が `touches` に入るかを試す。
fn add_footage_sequence(p: &mut Project) {
    let source = Node::footage(&Id::new("a1"));
    p.sequences.push(Sequence {
        id: Id::new("s2"),
        cuts: vec![Cut::new("kf", source, secs(0), secs(2))],
        origin: None,
    });
}

/// 素材 a1 と、その上の選別 sel1 と話題 t1。
fn add_bins(p: &mut Project) {
    let asset = Id::new("a1");
    p.assets.push(mm_doc::Asset {
        id: asset.clone(),
        path: "a1.mp4".into(),
        content_hash: "hash-a1".into(),
        color_space: mm_types::ColorSpace::bt709(),
        duration: secs(20),
        fps: Some(Rational::integer(30)),
        timebase_offset: RationalTime::ZERO,
    });
    p.bins.selections.push(mm_doc::Selection {
        id: Id::new("sel1"),
        asset: asset.clone(),
        in_point: secs(0),
        out_point: secs(2),
        rating: mm_doc::Rating::Favorite,
        tags: vec!["a".into()],
    });
    p.bins.topics.push(mm_doc::Topic {
        id: Id::new("t1"),
        asset,
        in_point: secs(1),
        out_point: secs(4),
        title: "話題".into(),
        rating: mm_doc::Rating::Unrated,
        tags: Vec::new(),
    });
}

/// 比べるための形。ID で引く列を並べ、JSON にする。
pub fn canon(p: &Project) -> serde_json::Value {
    let mut p = p.clone();
    p.normalize();
    serde_json::to_value(p).unwrap()
}
