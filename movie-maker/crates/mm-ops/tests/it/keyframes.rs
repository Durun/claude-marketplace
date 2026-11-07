//! キーフレームの操作（Inspector と Graph が出す形）。

use crate::common;

use common::{canon, project, quarters, secs};
use mm_doc::{CubicBezier, Interpolation, Keyframe, Project};
use mm_ops::{Editor, MoveKeyframe, Op, OpContext, ParamRef, Transaction};
use mm_types::{Id, ParamValue};

fn opacity() -> ParamRef {
    ParamRef::layer(&Id::new("l2"), "opacity")
}

fn key(t: i64, v: f64) -> Keyframe {
    Keyframe::new(secs(t), ParamValue::Number(v), Interpolation::Linear)
}

fn moved(from: i64, to: Keyframe) -> Op {
    MoveKeyframe {
        target: opacity(),
        from: secs(from),
        keyframe: to,
    }
    .into()
}

fn keys(p: &Project) -> Vec<(i64, f64)> {
    let (_, l) = p.layer(&Id::new("l2")).unwrap();
    l.opacity
        .keyframes
        .iter()
        .map(|k| (k.time.to_f64() as i64, k.value.as_number().unwrap()))
        .collect()
}

#[test]
fn 時刻と値を同時に動かし_時刻の順に並べ直す() {
    let mut p = project();
    let human = OpContext::human();
    moved(0, key(3, 0.5)).apply(&mut p, &human).unwrap();
    assert_eq!(keys(&p), vec![(2, 1.0), (3, 0.5)]);
}

#[test]
fn 区間の形も置き直せる() {
    let mut p = project();
    let mut k = key(0, 0.0);
    k.interpolation = Interpolation::Bezier;
    k.bezier = Some(CubicBezier {
        x1: 0.1,
        y1: 0.8,
        x2: 0.3,
        y2: 1.0,
    });
    moved(0, k.clone())
        .apply(&mut p, &OpContext::human())
        .unwrap();
    let (_, l) = p.layer(&Id::new("l2")).unwrap();
    assert_eq!(l.opacity.keyframes[0], k);
}

#[test]
fn 秒の途中の時刻へも動かせる() {
    let mut p = project();
    let to = Keyframe::new(quarters(5), ParamValue::Number(0.5), Interpolation::Linear);
    moved(0, to).apply(&mut p, &OpContext::human()).unwrap();
    let (_, l) = p.layer(&Id::new("l2")).unwrap();
    assert_eq!(l.opacity.keyframes[0].time, quarters(5));
}

#[test]
fn 別のキーフレームのある時刻へは動かさない() {
    let mut p = project();
    let before = canon(&p);
    let err = moved(0, key(2, 0.5)).apply(&mut p, &OpContext::human());
    assert!(err.is_err());
    assert_eq!(canon(&p), before);
}

#[test]
fn 無い時刻のキーフレームは動かさない() {
    let mut p = project();
    assert!(moved(1, key(3, 0.5))
        .apply(&mut p, &OpContext::human())
        .is_err());
}

#[test]
fn ドラッグの確定は取り消し_1_回で戻る() {
    let p = project();
    let before = canon(&p);
    let mut editor = Editor::new(p);
    let human = OpContext::human();
    editor
        .apply(
            Transaction::new("graph drag", vec![moved(0, key(3, 0.25))]),
            &human,
        )
        .unwrap();
    assert_eq!(keys(editor.project()), vec![(2, 1.0), (3, 0.25)]);
    editor.undo(&human).unwrap();
    assert_eq!(canon(editor.project()), before);
}
