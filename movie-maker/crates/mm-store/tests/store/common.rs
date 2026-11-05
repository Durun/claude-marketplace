//! テスト共通の文書と命令の作り方。

use serde_json::json;

use mm_doc::{Asset, Composition, Layer, Marker, Node, Project};
use mm_store::{Actor, OpRecord, TxnInput};
use mm_types::{ColorSpace, Id, ParamValue, Rational, RationalTime};

pub const NOW: i64 = 1_793_880_000_000;

pub fn layer(i: usize) -> Layer {
    let node = Node::new("text", 1).with_param("text", ParamValue::Text(format!("字幕その{i}")));
    Layer::new(
        format!("l{i}"),
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(1),
    )
}

pub fn project_with(layers: usize) -> Project {
    let mut p = Project::new();
    p.assets.push(Asset {
        id: Id::new("a1"),
        path: "D:/rec/収録01.mkv".into(),
        content_hash: "abc123".into(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(60),
        fps: Some(Rational::integer(30)),
        timebase_offset: RationalTime::ZERO,
    });
    p.compositions.push(Composition {
        id: Id::new("c"),
        width: 320,
        height: 180,
        fps: Rational::integer(30),
        duration: RationalTime::seconds(10),
        layers: (0..layers).map(layer).collect(),
        markers: vec![Marker {
            id: Id::new("m1"),
            time: RationalTime::seconds(2),
            duration: RationalTime::ZERO,
            name: "オープニングの章".into(),
        }],
    });
    p
}

pub fn op_record(i: usize) -> OpRecord {
    OpRecord {
        op: json!({ "type": "addLayer", "id": format!("l{i}"), "pad": "x".repeat(300) }),
        inverse: json!({ "type": "removeLayer", "id": format!("l{i}") }),
        touches: vec![format!("layer:c/l{i}")],
    }
}

pub fn txn(id: &str, ops: &[usize]) -> TxnInput {
    TxnInput {
        id: id.into(),
        name: "テスト".into(),
        actor: Actor {
            kind: "cli".into(),
            name: "test".into(),
        },
        at_ms: NOW,
        responds_to: None,
        ops: ops.iter().map(|i| op_record(*i)).collect(),
    }
}
