//! Mixer の操作（`setLayerAudio`、`setMixer`）。取り消し 1 回で前のプロジェクトのデータにバイトで戻る。

mod common;

use common::{canon, project};
use mm_doc::{CompMixer, Ducking, LayerAudio, MixerBus, Param};
use mm_ops::{Editor, Op, OpContext, SetLayerAudio, SetMixer, Transaction};
use mm_types::{Id, ParamValue};

fn audio(gain_db: f64, mute: bool, solo: bool) -> LayerAudio {
    LayerAudio {
        role: "bgm".into(),
        gain_db: Param::constant(ParamValue::Number(gain_db)),
        pan: Param::constant(ParamValue::Number(-0.5)),
        mute,
        solo,
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
