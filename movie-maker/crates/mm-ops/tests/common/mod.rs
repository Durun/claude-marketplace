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
    Composition {
        id: Id::new(id),
        width: 320,
        height: 180,
        fps: Rational::integer(30),
        duration: secs(10),
        layers,
        markers,
    }
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
    });
    p.validate().unwrap();
    p
}

/// 比べるための形。ID で引く列を並べ、JSON にする。
pub fn canon(p: &Project) -> serde_json::Value {
    let mut p = p.clone();
    p.normalize();
    serde_json::to_value(p).unwrap()
}
