//! テスト共通のプロジェクトのデータと操作の作り方。操作の記録は実際の `mm_ops::Op` を適用して作る。

use mm_doc::{Asset, Composition, Layer, Marker, Node, Project};
use mm_ops::{apply_transaction, LockLayer, Op, OpContext, OpRecord, Touches};
use mm_store::{Actor, TxnInput};
use mm_types::{ColorSpace, Id, LayerId, ParamValue, Rational, RationalTime};

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
    p.compositions.push(
        Composition::new(
            Id::new("c"),
            320,
            180,
            Rational::integer(30),
            RationalTime::seconds(10),
        )
        .with_layers((0..layers).map(layer).collect())
        .with_markers(vec![Marker {
            id: Id::new("m1"),
            time: RationalTime::seconds(2),
            duration: RationalTime::ZERO,
            name: "オープニングの章".into(),
        }]),
    );
    p
}

pub fn actor() -> Actor {
    Actor {
        kind: "cli".into(),
        name: "test".into(),
    }
}

/// レイヤー `l{i}` のロックを反転する操作。
pub fn toggle_lock(project: &Project, i: usize) -> Op {
    let id = LayerId::new(format!("l{i}"));
    let lock = project
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .find(|l| l.id == id)
        .is_some_and(|l| !l.lock);
    LockLayer { layer: id, lock }.into()
}

/// 全レイヤーのロックを `lock` にする操作の記録を、適用せずに作る（kill の試験で、書き込みだけの子を速く回すため）。
pub fn lock_records(layers: usize, lock: bool) -> Vec<OpRecord> {
    (0..layers)
        .map(|i| {
            let id = LayerId::new(format!("l{i}"));
            let op: Op = LockLayer {
                layer: id.clone(),
                lock,
            }
            .into();
            let inverse: Op = LockLayer {
                layer: id.clone(),
                lock: !lock,
            }
            .into();
            OpRecord::new(&op, &inverse, &Touches::of(&id)).unwrap()
        })
        .collect()
}

/// `project` に（互いに違う）レイヤーのロックの反転を適用し、その記録を持つトランザクションを作る。
/// `project` は適用した後のプロジェクトのデータになる。
pub fn locked(project: &mut Project, name: &str, layers: &[usize]) -> TxnInput {
    let ops: Vec<Op> = layers.iter().map(|&i| toggle_lock(project, i)).collect();
    let outcome = apply_transaction(project, &ops, &OpContext::human()).unwrap();
    TxnInput::normal(name, actor(), NOW, outcome.records)
}
