//! 大きなプロジェクトのデータでの操作 1 つの時間。予算は `docs/design/17-instrumentation.md` の「操作 1 つ 20 ms」。
//! 時間は release のときだけ確かめる（debug は桁が違う）。数字は `cargo test --release --test it perf -- --nocapture` で出る。

use std::time::{Duration, Instant};

use crate::common::{comp, cut, layer};
use mm_doc::{Node, Project, Sequence, NESTED_KIND, NESTED_PARAM};
use mm_ops::*;
use mm_types::{Id, ParamValue};

const LAYERS: usize = 2000;
const BUDGET: Duration = Duration::from_millis(20);
const REPEAT: u32 = 20;

const CUTS: usize = 200;
const LEAF_COMPS: usize = 50;

/// レイヤー 2000 個。5 個ごとに前のレイヤーを親にし（鎖の長さは 5）、4 個に 1 個はカットに接続し、
/// 10 個に 1 個は葉のコンポジションを入れ子にする。
fn big_project() -> Project {
    let layers = (0..LAYERS)
        .map(|i| {
            let mut l = layer(&format!("l{i}"), 0, 0, 2);
            if i % 5 != 0 {
                l.parent = Some(Id::new(format!("l{}", i - 1)));
            }
            if i % 4 == 0 {
                l.attach_to = Some(Id::new(format!("k{}", i % CUTS)));
            }
            if i % 10 == 0 {
                l.source = nested(&format!("leaf{}", i % LEAF_COMPS));
            }
            l
        })
        .collect();
    let mut p = Project::new();
    p.compositions.push(comp("c", layers, Vec::new()));
    for i in 0..LEAF_COMPS {
        p.compositions
            .push(comp(&format!("leaf{i}"), Vec::new(), Vec::new()));
    }
    p.sequences.push(Sequence {
        id: Id::new("s"),
        cuts: (0..CUTS)
            .map(|i| cut(&format!("k{i}"), 0, 2, (1, 1)))
            .collect(),
        origin: None,
    });
    p.normalize();
    p.validate().unwrap();
    p
}

fn nested(target: &str) -> Node {
    Node::new(NESTED_KIND, 1).with_param(NESTED_PARAM, ParamValue::Text(target.into()))
}

fn mean(mut f: impl FnMut()) -> Duration {
    let start = Instant::now();
    for _ in 0..REPEAT {
        f();
    }
    start.elapsed() / REPEAT
}

fn check(name: &str, took: Duration) {
    println!("{name}: {took:?}（予算 {BUDGET:?}）");
    if !cfg!(debug_assertions) {
        assert!(took <= BUDGET, "{name} が予算を超えた: {took:?}");
    }
}

#[test]
fn レイヤー_2000_個のプロジェクトのデータで操作_1_つが予算に収まる() {
    let p = big_project();
    let human = OpContext::human();
    check("validate", mean(|| p.validate().unwrap()));
    let set_opacity: Op = SetParam {
        target: ParamRef::layer(&Id::new("l1500"), "opacity"),
        value: ParamValue::Number(0.5),
        keyframes: None,
    }
    .into();
    check(
        "setParam",
        mean(|| {
            set_opacity.apply(&mut p.clone(), &human).unwrap();
        }),
    );
    let reparent: Op = ParentLayer {
        layer: Id::new("l1999"),
        parent: Some(Id::new("l1000")),
    }
    .into();
    check(
        "parentLayer",
        mean(|| {
            reparent.apply(&mut p.clone(), &human).unwrap();
        }),
    );
    let add: Op = AddLayer {
        composition: Id::new("c"),
        layer: layer("new", 0, 0, 2),
        index: None,
    }
    .into();
    check(
        "addLayer",
        mean(|| {
            add.apply(&mut p.clone(), &human).unwrap();
        }),
    );
}
