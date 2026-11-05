//! トランザクション、版番号、同時書き込みの判定、ロック、取り消しとやり直し。

mod common;

use common::{canon, cut, layer, project, quarters};
use mm_doc::Project;
use mm_ops::{
    AddLayer, Disable, Edge, Editor, Insert, LockLayer, Op, OpContext, OpError, Precompose,
    RemoveLayer, Ripple, SetParam, Slip, Transaction, TxnKind,
};
use mm_types::{Id, ParamValue};

fn ripple(cut: &str, by: i64) -> Op {
    Ripple {
        cut: Id::new(cut),
        edge: Edge::Out,
        by: quarters(by),
    }
    .into()
}

fn set_opacity(layer: &str, v: f64) -> Op {
    SetParam {
        target: mm_ops::ParamRef::layer(&Id::new(layer), "opacity"),
        value: ParamValue::Number(v),
        keyframes: None,
    }
    .into()
}

fn missing_layer() -> Op {
    RemoveLayer {
        layer: Id::new("nope"),
    }
    .into()
}

#[test]
fn 途中で失敗したトランザクションは文書を変えない() {
    let mut p = project();
    let before = canon(&p);
    let ops = vec![ripple("k1", 1), set_opacity("l1", 0.5), missing_layer()];
    let err = mm_ops::apply_transaction(&mut p, &ops, &OpContext::human()).unwrap_err();
    assert!(matches!(err, OpError::Doc(_)));
    assert_eq!(canon(&p), before);
}

#[test]
fn 版は命令の数だけ進み記録に残る() {
    let mut ed = Editor::new(project());
    let txn = Transaction::new("編集", vec![ripple("k1", 1), set_opacity("l1", 0.5)]);
    let rec = ed.apply(txn, &OpContext::human()).unwrap();
    assert_eq!(rec.id, 1);
    assert_eq!(rec.base_version, 0);
    assert_eq!(rec.version, 2);
    assert_eq!(rec.ops.len(), 2);
    assert_eq!(rec.ops[0].op["op"], "ripple");
    assert_eq!(rec.ops[0].inverse["op"], "ripple");
    assert_eq!(rec.ops[0].touches, vec!["k1".to_owned()]);
    assert_eq!(rec.op_version(1), 2);
    assert_eq!(ed.version(), 2);
}

#[test]
fn 古い版の命令は触る所が重ならなければ当たり重なれば断る() {
    let mut ed = Editor::new(project());
    let human = OpContext::human();
    ed.apply(Transaction::new("a", vec![ripple("k1", 1)]), &human)
        .unwrap();
    // 版 0 を元にした、k3 だけ触る命令は当たる。
    ed.apply(
        Transaction::new("b", vec![ripple("k3", 1)]).based_on(0),
        &human,
    )
    .unwrap();
    // 版 0 を元にした、k1 を触る命令は断る。文書は変わらない。
    let before = canon(ed.project());
    let err = ed
        .apply(
            Transaction::new("c", vec![set_opacity("l1", 0.2), ripple("k1", 1)]).based_on(0),
            &human,
        )
        .unwrap_err();
    match err {
        OpError::Conflict { base, touches } => {
            assert_eq!(base, 0);
            assert_eq!(touches.to_vec(), vec!["k1".to_owned()]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(canon(ed.project()), before);
    assert_eq!(ed.version(), 2);
    // 先の版を元にした命令は断る。
    let err = ed
        .apply(
            Transaction::new("d", vec![ripple("k2", 1)]).based_on(9),
            &human,
        )
        .unwrap_err();
    assert!(matches!(
        err,
        OpError::FutureBase {
            base: 9,
            current: 2
        }
    ));
}

#[test]
fn ロックの付いたレイヤーは_claude_の入口から触れない() {
    let mut p = project();
    let human = OpContext::human();
    let claude = OpContext::claude();
    LockLayer {
        layer: Id::new("l1"),
        lock: true,
    }
    .apply_op(&mut p, &human);
    let err = set_opacity("l1", 0.5).apply(&mut p, &claude).unwrap_err();
    assert!(matches!(err, OpError::Locked(_)));
    // 外すのも断る。
    let unlock: Op = LockLayer {
        layer: Id::new("l1"),
        lock: false,
    }
    .into();
    assert!(matches!(
        unlock.apply(&mut p, &claude),
        Err(OpError::Locked(_))
    ));
    // ほかのレイヤーは触れる。人は触れる。
    set_opacity("l2", 0.5).apply(&mut p, &claude).unwrap();
    set_opacity("l1", 0.5).apply(&mut p, &human).unwrap();
    // ロックのレイヤーを巻き込む precompose も断る。
    let pre: Op = Precompose {
        composition: Id::new("c1"),
        layers: vec![Id::new("l1"), Id::new("l2")],
        new_composition: Id::new("cx"),
        new_layer: Id::new("lx"),
    }
    .into();
    assert!(matches!(
        pre.apply(&mut p, &claude),
        Err(OpError::Locked(_))
    ));
}

trait ApplyOp {
    fn apply_op(self, p: &mut Project, ctx: &OpContext);
}

impl<T: Into<Op>> ApplyOp for T {
    fn apply_op(self, p: &mut Project, ctx: &OpContext) {
        self.into().apply(p, ctx).unwrap();
    }
}

#[test]
fn 取り消しとやり直しはまとまりの単位() {
    let mut ed = Editor::new(project());
    let human = OpContext::human();
    let v0 = canon(ed.project());
    ed.apply(
        Transaction::new("a", vec![ripple("k1", 1), set_opacity("l1", 0.5)]),
        &human,
    )
    .unwrap();
    let v1 = canon(ed.project());
    ed.apply(Transaction::new("b", vec![slip("k2", 1)]), &human)
        .unwrap();
    let v2 = canon(ed.project());
    assert_eq!(ed.version(), 3);

    let undo = ed.undo(&human).unwrap().unwrap();
    assert_eq!(undo.kind, TxnKind::Undo { of: 2 });
    assert_eq!(undo.version, 4);
    assert_eq!(canon(ed.project()), v1);
    let undo = ed.undo(&human).unwrap().unwrap();
    assert_eq!(undo.kind, TxnKind::Undo { of: 1 });
    assert_eq!(undo.ops.len(), 2);
    assert_eq!(undo.ops[0].op["op"], "setParam");
    assert_eq!(canon(ed.project()), v0);
    assert!(!ed.can_undo());
    assert!(ed.undo(&human).unwrap().is_none());

    let redo = ed.redo(&human).unwrap().unwrap();
    assert_eq!(redo.kind, TxnKind::Redo { of: 1 });
    assert_eq!(canon(ed.project()), v1);
    ed.redo(&human).unwrap().unwrap();
    assert_eq!(canon(ed.project()), v2);
    assert!(!ed.can_redo());

    // 取り消した後に新しい命令を当てると、やり直しの先は捨てる。
    ed.undo(&human).unwrap();
    ed.apply(Transaction::new("c", vec![ripple("k4", 1)]), &human)
        .unwrap();
    assert!(!ed.can_redo());
    assert!(ed.can_undo());
    assert_eq!(ed.log().len(), 8);
}

fn slip(cut: &str, by: i64) -> Op {
    Slip {
        cut: Id::new(cut),
        by: quarters(by),
    }
    .into()
}

#[test]
fn 命令は_json_を往復し名前が_op_の欄に出る() {
    let op: Op = Insert {
        sequence: Id::new("s1"),
        index: 1,
        cut: cut("kx", 0, 1, (1, 1)),
    }
    .into();
    let v = serde_json::to_value(&op).unwrap();
    assert_eq!(v["op"], "insert");
    assert_eq!(v["index"], 1);
    let back: Op = serde_json::from_value(v).unwrap();
    assert_eq!(back, op);
    assert_eq!(back.name(), "insert");
    let d: Op = serde_json::from_str(r#"{"op":"disable","cut":"k1"}"#).unwrap();
    assert_eq!(
        d,
        Disable {
            cut: Id::new("k1"),
            disabled: true
        }
        .into()
    );
    assert!(serde_json::from_str::<Op>(r#"{"op":"disable","cut":"k1","x":1}"#).is_err());
}

#[test]
fn 適用の後は_validate_が通り_逆命令で戻る() {
    let mut p = project();
    let human = OpContext::human();
    let op: Op = AddLayer {
        composition: Id::new("c2"),
        layer: layer("lx", 0, 0, 3),
        index: None,
    }
    .into();
    let before = canon(&p);
    let applied = op.apply(&mut p, &human).unwrap();
    assert_eq!(
        applied.touches.to_vec(),
        vec!["c2".to_owned(), "lx".to_owned()]
    );
    p.validate().unwrap();
    applied.inverse.apply(&mut p, &human).unwrap();
    assert_eq!(canon(&p), before);
}
