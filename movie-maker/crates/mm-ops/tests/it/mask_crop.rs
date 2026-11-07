//! マスク（`addMask`、`removeMask`、`masks.<ID>.rect.*`）、切り抜き（`crop.*`）、音のフェード（`setLayerAudio`）。
//! 取り消し 1 回で前のプロジェクトのデータにバイトで戻る。

use mm_doc::{
    Composition, Fade, FadeCurve, Interpolation, Keyframe, Layer, LayerAudio, Mask, MaskRect, Node,
    Project,
};
use mm_ops::{
    AddMask, Editor, Op, OpContext, ParamRef, RemoveMask, SetKeyframe, SetLayerAudio, SetParam,
    Transaction,
};
use mm_types::{Id, ParamValue, Rational, RationalTime};

/// コンポジション `c1` に、レイヤー `l1` が 1 つ。
fn project() -> Project {
    let secs = RationalTime::seconds;
    let mut p = Project::new();
    p.compositions.push(
        Composition::new(Id::new("c1"), 320, 180, Rational::integer(30), secs(10)).with_layers(
            vec![Layer::new(
                "l1",
                Node::new("solid", 1),
                secs(0),
                secs(0),
                secs(10),
            )],
        ),
    );
    p
}

fn canon(p: &Project) -> serde_json::Value {
    serde_json::to_value(p).unwrap()
}

fn layer(ed: &Editor) -> &Layer {
    ed.project().layer(&Id::new("l1")).unwrap().1
}

fn add_mask(id: &str, index: Option<usize>) -> Op {
    AddMask {
        layer: Id::new("l1"),
        mask: Mask::rect(id, MaskRect::new([10.0, 10.0], [100.0, 60.0], 8.0)),
        index,
    }
    .into()
}

/// 操作の列を 1 つのトランザクションで適用し、取り消して元に戻ることを確かめる。
fn apply_and_undo(before: &Project, ops: Vec<Op>) -> Editor {
    let mut ed = Editor::new(before.clone());
    let ctx = OpContext::human();
    ed.apply(Transaction::new("edit", ops), &ctx).unwrap();
    let after = canon(ed.project());
    ed.undo(&ctx).unwrap();
    assert_eq!(canon(ed.project()), canon(before), "取り消すと元に戻る");
    ed.redo(&ctx).unwrap();
    assert_eq!(canon(ed.project()), after, "やり直すと同じになる");
    ed
}

#[test]
fn マスクを足して角の半径にキーフレームを置き_取り消すと元に戻る() {
    let radius = ParamRef::layer(&Id::new("l1"), "masks.m1.rect.radius");
    let key = Keyframe::new(
        RationalTime::seconds(1),
        ParamValue::Number(20.0),
        Interpolation::Linear,
    );
    let ed = apply_and_undo(
        &project(),
        vec![
            add_mask("m1", None),
            SetKeyframe {
                target: radius,
                keyframe: key,
            }
            .into(),
        ],
    );
    let mask = &layer(&ed).masks[0];
    assert_eq!(mask.rect.as_ref().unwrap().radius.keyframes.len(), 1);
}

#[test]
fn マスクを外すと取り消しで元の位置に戻る() {
    let mut ed = Editor::new(project());
    let ctx = OpContext::human();
    let ops = vec![
        add_mask("a", None),
        add_mask("b", None),
        add_mask("c", None),
    ];
    ed.apply(Transaction::new("masks", ops), &ctx).unwrap();
    let before = ed.project().clone();
    let remove: Op = RemoveMask { mask: Id::new("b") }.into();
    let ed = apply_and_undo(&before, vec![remove]);
    let ids: Vec<&str> = layer(&ed).masks.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["a", "c"]);
}

#[test]
fn マスクの位置が数を超えると拒否する() {
    let mut ed = Editor::new(project());
    let r = ed.apply(
        Transaction::new("bad", vec![add_mask("m1", Some(1))]),
        &OpContext::human(),
    );
    assert!(r.is_err());
}

#[test]
fn 切り抜きの辺を置くと作られ_取り消すと切り抜きの無い元に戻る() {
    let before = project();
    let set: Op = SetParam {
        target: ParamRef::layer(&Id::new("l1"), "crop.left"),
        value: ParamValue::Number(40.0),
        keyframes: None,
    }
    .into();
    let ed = apply_and_undo(&before, vec![set]);
    let crop = &layer(&ed).crop;
    assert_eq!(crop.left.as_ref().unwrap().value.as_number(), Some(40.0));
    assert!(crop.right.is_none());
    assert!(
        canon(&before)
            .pointer("/compositions/0/layers/0/crop")
            .is_none(),
        "切り抜きの無いレイヤーは JSON に crop を書かない"
    );
}

#[test]
fn フェードの曲線を選んで置き_取り消すと元に戻る() {
    let audio = LayerAudio {
        fade_in: Some(Fade {
            duration: RationalTime::new(1, 2).unwrap(),
            curve: FadeCurve::EqualPower,
        }),
        fade_out: Some(Fade::linear(RationalTime::seconds(1))),
        ..LayerAudio::new("bgm")
    };
    let set: Op = SetLayerAudio {
        layer: Id::new("l1"),
        audio: Some(audio.clone()),
    }
    .into();
    let ed = apply_and_undo(&project(), vec![set]);
    assert_eq!(layer(&ed).audio.as_ref(), Some(&audio));
}

#[test]
fn 長さが_0_のフェードは検証で拒否する() {
    let set: Op = SetLayerAudio {
        layer: Id::new("l1"),
        audio: Some(LayerAudio {
            fade_in: Some(Fade::linear(RationalTime::ZERO)),
            ..LayerAudio::new("bgm")
        }),
    }
    .into();
    let mut ed = Editor::new(project());
    let r = ed.apply(Transaction::new("bad", vec![set]), &OpContext::human());
    assert!(r.is_err());
}
