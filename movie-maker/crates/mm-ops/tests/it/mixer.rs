//! Mixer の操作（`setLayerAudio`、`setMixer`）。取り消し 1 回で前のプロジェクトのデータにバイトで戻る。

use mm_doc::{CompMixer, Composition, Ducking, Layer, LayerAudio, MixerBus, Node, Param, Project};
use mm_ops::{
    Editor, Op, OpContext, Precompose, SetLayerAudio, SetMixer, Transaction, Unprecompose,
};
use mm_types::{Id, ParamValue, Rational, RationalTime};

/// コンポジション `c1` に、音の設定の無いレイヤー `l1` が 1 つ。
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

fn audio(gain_db: f64, mute: bool, solo: bool) -> LayerAudio {
    LayerAudio {
        gain_db: Param::constant(ParamValue::Number(gain_db)),
        pan: Param::constant(ParamValue::Number(-0.5)),
        mute,
        solo,
        ..LayerAudio::new("bgm")
    }
}

fn set_audio(layer: &str, a: Option<LayerAudio>) -> Op {
    SetLayerAudio {
        layer: Id::new(layer),
        audio: a,
    }
    .into()
}

fn ducking_mixer() -> CompMixer {
    CompMixer {
        master_db: -3.0,
        buses: [("bgm".to_owned(), MixerBus { gain_db: -6.0 })].into(),
        ducking: Some(Ducking::voice_over_bgm()),
    }
}

#[test]
fn 音の設定の無いレイヤーに置いて取り消すと設定の無い元に戻る() {
    let before = project();
    let mut ed = Editor::new(before.clone());
    let ctx = OpContext::human();
    let op = set_audio("l1", Some(audio(-6.0, true, false)));
    ed.apply(Transaction::new("mute", vec![op]), &ctx).unwrap();
    let a = ed.project().layer(&Id::new("l1")).unwrap().1.audio.clone();
    assert_eq!(a, Some(audio(-6.0, true, false)));
    ed.undo(&ctx).unwrap();
    assert_eq!(canon(ed.project()), canon(&before));
}

#[test]
fn ミキサーの設定を置いて取り消すと既定に戻り_やり直すと戻る() {
    let before = project();
    let mut ed = Editor::new(before.clone());
    let ctx = OpContext::human();
    let op: Op = SetMixer {
        composition: Id::new("c1"),
        mixer: Some(ducking_mixer()),
    }
    .into();
    ed.apply(Transaction::new("ducking", vec![op]), &ctx)
        .unwrap();
    let c1 = ed.project().composition(&Id::new("c1")).unwrap();
    assert_eq!(c1.mixer, Some(ducking_mixer()));
    ed.undo(&ctx).unwrap();
    assert_eq!(canon(ed.project()), canon(&before));
    ed.redo(&ctx).unwrap();
    let c1 = ed.project().composition(&Id::new("c1")).unwrap();
    assert_eq!(c1.mixer, Some(ducking_mixer()));
}

#[test]
fn 有限でない音量のミキサーは検証で拒否し_データを変えない() {
    let before = project();
    let mut ed = Editor::new(before.clone());
    let op: Op = SetMixer {
        composition: Id::new("c1"),
        mixer: Some(CompMixer {
            master_db: f64::NAN,
            ..CompMixer::default()
        }),
    }
    .into();
    let r = ed.apply(Transaction::new("bad", vec![op]), &OpContext::human());
    assert!(r.is_err());
    assert_eq!(canon(ed.project()), canon(&before));
}

#[test]
fn ミキサーを持つ入れ子は_unprecompose_で戻さない() {
    // 戻すとミキサーの設定が消え、逆操作の precompose で元のプロジェクトのデータに戻らないため。
    let mut ed = Editor::new(project());
    let ctx = OpContext::human();
    let pre: Op = Precompose {
        composition: Id::new("c1"),
        layers: vec![Id::new("l1")],
        new_composition: Id::new("inner"),
        new_layer: Id::new("nest"),
    }
    .into();
    let mix: Op = SetMixer {
        composition: Id::new("inner"),
        mixer: Some(ducking_mixer()),
    }
    .into();
    ed.apply(Transaction::new("nest", vec![pre, mix]), &ctx)
        .unwrap();
    let before = canon(ed.project());
    let un: Op = Unprecompose {
        layer: Id::new("nest"),
    }
    .into();
    assert!(ed.apply(Transaction::new("un", vec![un]), &ctx).is_err());
    assert_eq!(canon(ed.project()), before);
}

#[test]
fn json_の名前は_camel_case() {
    let op = set_audio("l1", None);
    let v = serde_json::to_value(&op).unwrap();
    assert_eq!(v["op"], "setLayerAudio");
    let back: Op = serde_json::from_value(serde_json::json!({
        "op": "setMixer", "composition": "c1",
        "mixer": {"master_db": -1.0, "ducking": {
            "trigger": "voice", "targets": ["bgm"], "reduction_db": -9.0,
            "threshold_db": -40.0, "attack_ms": 10.0, "release_ms": 200.0
        }}
    }))
    .unwrap();
    assert!(matches!(back, Op::SetMixer(_)));
}
