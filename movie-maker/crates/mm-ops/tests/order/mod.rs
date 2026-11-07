//! 順の入れ替えの性質: `touches` が重ならない 2 つの操作は、別々に 1 つずつ適用すると、順を変えても結果が同じ。
//! レビューで再現した、順で結果が変わるプロジェクトのデータを固定する。

use crate::common::{self, canon, comp, layer, project};
use mm_doc::{Cut, Layer, Node, Project, NESTED_KIND, NESTED_PARAM};
use mm_ops::*;
use mm_types::{Id, ParamValue, RationalTime};

/// 操作を 1 つずつ別に適用する。適用できない操作はスキップする。各操作の成否と最後のプロジェクトのデータを返す。
fn apply_each(p: &Project, ops: &[&Op]) -> (Vec<bool>, serde_json::Value) {
    let mut doc = p.clone();
    let human = OpContext::human();
    let oks = ops
        .iter()
        .map(|op| op.apply(&mut doc, &human).is_ok())
        .collect();
    (oks, canon(&doc))
}

/// `a` と `b` を両方の順で別々に適用し、`touches` が重ならないのに成否か結果が変わるなら理由を返す。
pub fn order_violation(p: &Project, a: &Op, b: &Op) -> Option<String> {
    let (ta, tb) = (a.touches(p).ok()?, b.touches(p).ok()?);
    if ta.overlaps(&tb) {
        return None;
    }
    let (ok_ab, doc_ab) = apply_each(p, &[a, b]);
    let (ok_ba, doc_ba) = apply_each(p, &[b, a]);
    if (ok_ab[0], ok_ab[1]) != (ok_ba[1], ok_ba[0]) {
        return Some(format!(
            "成否が順で変わる: a={a:?} b={b:?} ab={ok_ab:?} ba={ok_ba:?}"
        ));
    }
    (doc_ab != doc_ba).then(|| format!("結果が順で変わる: a={a:?} b={b:?}"))
}

fn assert_order_free(p: &Project, a: &Op, b: &Op) {
    if let Some(why) = order_violation(p, a, b) {
        panic!("{why}");
    }
}

fn parent(l: &str, parent: &str) -> Op {
    ParentLayer {
        layer: Id::new(l),
        parent: Some(Id::new(parent)),
    }
    .into()
}

fn with_layers(parents: &[(&str, Option<&str>)]) -> Project {
    let layers = parents
        .iter()
        .map(|(id, par)| {
            let mut l = layer(id, 0, 0, 2);
            l.parent = par.map(Id::new);
            l
        })
        .collect();
    let mut p = Project::new();
    p.compositions.push(comp("c", layers, vec![]));
    p.validate().unwrap();
    p
}

#[test]
fn parent_layer_は新しい親の祖先の鎖を_touches_に含める() {
    // Y.parent = U、V.parent = X。X→Y と U→V は、どちらも単独なら通るが、
    // 片方を適用した後のもう片方は循環で拒否される。
    let p = with_layers(&[("x", None), ("y", Some("u")), ("u", None), ("v", Some("x"))]);
    let (a, b) = (parent("x", "y"), parent("u", "v"));
    assert!(apply_each(&p, &[&a]).0[0] && apply_each(&p, &[&b]).0[0]);
    assert_order_free(&p, &a, &b);
}

fn nested_layer(id: &str, target: &str) -> Layer {
    let node = Node::new(NESTED_KIND, 1).with_param(NESTED_PARAM, ParamValue::Text(target.into()));
    Layer::new(
        id,
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(1),
    )
}

fn add_layer(comp: &str, layer: Layer) -> Op {
    AddLayer {
        composition: Id::new(comp),
        layer,
        index: None,
    }
    .into()
}

/// cB は cC を入れ子にし、cD は cA を入れ子にする。
fn nesting_chain() -> Project {
    let mut p = Project::new();
    p.compositions.push(comp("cA", vec![], vec![]));
    p.compositions
        .push(comp("cB", vec![nested_layer("lbc", "cC")], vec![]));
    p.compositions.push(comp("cC", vec![], vec![]));
    p.compositions
        .push(comp("cD", vec![nested_layer("lda", "cA")], vec![]));
    p.validate().unwrap();
    p
}

#[test]
fn add_layer_が作る入れ子の循環は参照先の先を_touches_に含める() {
    // cA に cB を、cC に cD を入れ子にする。単独なら通るが、両方適用すると cA→cB→cC→cD→cA の循環。
    let p = nesting_chain();
    let a = add_layer("cA", nested_layer("x1", "cB"));
    let b = add_layer("cC", nested_layer("x2", "cD"));
    assert!(apply_each(&p, &[&a]).0[0] && apply_each(&p, &[&b]).0[0]);
    assert_order_free(&p, &a, &b);
}

#[test]
fn set_param_が作る入れ子の循環は参照先の先を_touches_に含める() {
    // lax（cA の中）と lcx（cC の中）は葉の cE を入れ子にする。
    // lax を cB へ、lcx を cD へ向ける。単独なら通るが、両方適用すると cA→cB→cC→cD→cA の循環。
    let mut p = nesting_chain();
    p.compositions.push(comp("cE", vec![], vec![]));
    p.compositions[0].layers.push(nested_layer("lax", "cE"));
    p.compositions[2].layers.push(nested_layer("lcx", "cE"));
    p.validate().unwrap();
    let set = |l: &str, target: &str| -> Op {
        SetParam {
            target: ParamRef::layer(&Id::new(l), "source.composition"),
            value: ParamValue::Text(target.into()),
            keyframes: None,
        }
        .into()
    };
    let (a, b) = (set("lax", "cB"), set("lcx", "cD"));
    assert!(apply_each(&p, &[&a]).0[0] && apply_each(&p, &[&b]).0[0]);
    assert_order_free(&p, &a, &b);
}

#[test]
fn ロックしたレイヤーが接続したカットを_claude_の操作経路から動かせない() {
    let mut p = project();
    p.compositions[0].layers[0].attach_to = Some(Id::new("k2"));
    p.compositions[0].layers[0].lock = true;
    p.validate().unwrap();
    let cli = OpContext::new(Entry::Cli);
    let cut = |c: &str| Id::new(c);
    let q = common::quarters(1);
    let ops: Vec<Op> = vec![
        Ripple {
            cut: cut("k1"),
            edge: Edge::Out,
            by: q,
        }
        .into(),
        Ripple {
            cut: cut("k2"),
            edge: Edge::Out,
            by: q,
        }
        .into(),
        Roll {
            cut: cut("k1"),
            by: q,
        }
        .into(),
        Slip {
            cut: cut("k2"),
            by: q,
        }
        .into(),
        Slide {
            cut: cut("k3"),
            by: q,
        }
        .into(),
        Disable {
            cut: cut("k2"),
            disabled: true,
        }
        .into(),
        Split {
            cut: cut("k2"),
            at: q,
            new_cut: cut("kn"),
        }
        .into(),
        Join {
            cut: cut("k1"),
            next: cut("k2"),
        }
        .into(),
    ];
    for op in ops {
        let mut doc = p.clone();
        let result = op.apply(&mut doc, &cli);
        assert!(
            matches!(result, Err(OpError::Locked(_))),
            "ロックを素通りした: {op:?}"
        );
    }
}

#[test]
fn cut_words_は素材を映す全部のカットを_touches_に含める() {
    // f の素材の範囲は [0, 2)。範囲 [2, 2.25) は掛からず、CutWords は単独だと「変わるものが無い」で拒否される。
    // Slip で f が [1, 3) に動いた後なら範囲が掛かって割れるので、順で成否が変わる。
    let a1 = Id::new("a1");
    let mut p = project();
    p.sequences[0].cuts.push(Cut::new(
        "f",
        Node::footage(&a1),
        common::secs(0),
        common::secs(2),
    ));
    p.validate().unwrap();
    let cut_words: Op = CutWords {
        asset: a1,
        sequence: Id::new("s1"),
        words: Vec::new(),
        ranges: vec![SourceRange {
            in_point: common::secs(2),
            out_point: common::quarters(9),
        }],
        restore: false,
        splices: Vec::new(),
    }
    .into();
    let slip: Op = Slip {
        cut: Id::new("f"),
        by: common::secs(1),
    }
    .into();
    assert!(!apply_each(&p, &[&cut_words]).0[0] && apply_each(&p, &[&slip]).0[0]);
    assert_order_free(&p, &cut_words, &slip);
}

#[test]
fn 同じ時刻のマーカーは_add_marker_が拒否し_remove_marker_の逆操作で戻る() {
    let mut p = project();
    let human = OpContext::human();
    let dup: Op = AddMarker {
        composition: Id::new("c1"),
        marker: common::marker("mdup", 1),
    }
    .into();
    assert!(dup.apply(&mut p, &human).is_err());
    let remove: Op = RemoveMarker {
        marker: Id::new("m1"),
    }
    .into();
    let before = canon(&p);
    let applied = remove.apply(&mut p, &human).unwrap();
    applied.inverse.apply(&mut p, &human).unwrap();
    assert_eq!(canon(&p), before);
}
