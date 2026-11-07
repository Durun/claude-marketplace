use std::sync::Arc;

use mm_audio_api::{DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_doc::{Composition, Interpolation, Keyframe, MixerBus, Node};
use mm_runtime::{assemble, Backend, RuntimeConfig};
use mm_types::{ColorSpace, Id, ParamValue, Rational, RationalTime};

use super::*;

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

fn audio(role: &str) -> LayerAudio {
    LayerAudio::new(role)
}

/// 下から、撮影素材（音の設定なし）、単色、入れ子（役割 `fx`、音量にキーフレーム）。BGM のバスは `music`。
fn doc() -> OpenDoc {
    let mut project = Project::new();
    project.assets.push(Asset {
        id: Id::new("a1"),
        path: "C:\\media\\talk.wav".into(),
        content_hash: "hash".into(),
        color_space: ColorSpace::bt709(),
        duration: secs(5),
        fps: None,
        timebase_offset: RationalTime::ZERO,
    });
    let footage =
        Node::new(FOOTAGE_KIND, 1).with_param(FOOTAGE_PARAM, ParamValue::AssetRef(Id::new("a1")));
    let nested =
        Node::new(NESTED_KIND, 1).with_param(NESTED_PARAM, ParamValue::Text("inner".into()));
    let mut nest = Layer::new("nest", nested, secs(0), secs(0), secs(5));
    let mut fx = audio("fx");
    fx.gain_db.keyframes = vec![Keyframe::new(
        secs(1),
        ParamValue::Number(-6.0),
        Interpolation::Linear,
    )];
    nest.audio = Some(fx);
    project.compositions.push(
        Composition::new(Id::new("main"), 16, 9, Rational::integer(30), secs(5))
            .with_layers(vec![
                Layer::new("talk", footage, secs(0), secs(0), secs(5)),
                Layer::new("bg", Node::new("solid", 1), secs(0), secs(0), secs(5)),
                nest,
            ])
            .with_mixer(CompMixer {
                buses: [("music".to_owned(), MixerBus { gain_db: -3.0 })].into(),
                ..CompMixer::default()
            }),
    );
    OpenDoc {
        project: Arc::new(project),
        comp: Id::new("main"),
        version: None,
    }
}

#[test]
fn トラックは音を持てるレイヤーだけを上から並べ_名前と役割を出す() {
    let s = state_of(&doc()).unwrap();
    let ids: Vec<&str> = s.tracks.iter().map(|t| t.layer.as_str()).collect();
    assert_eq!(ids, ["nest", "talk"], "単色は音を持てない");
    assert_eq!(s.tracks[0].label, "inner");
    assert!(s.tracks[0].animated);
    assert_eq!(s.tracks[1].label, "talk.wav");
    assert!(!s.tracks[1].animated && s.tracks[1].audio.is_none());
    assert_eq!(s.roles, ["voice", "bgm", "sfx", "fx", "music"]);
    assert_eq!(s.composition, "main");
}

fn tone(amp: f32, frames: usize) -> AudioBlock {
    let s = vec![amp; frames * DEFAULT_CHANNELS as usize];
    AudioBlock::from_samples(DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS, s).unwrap()
}

fn level(layer: &str, peak: f32) -> Vec<TrackLevel> {
    vec![TrackLevel {
        layer: Id::new(layer),
        peak,
    }]
}

#[test]
fn メーターは再生位置で鳴っている区間を選び_先読みの値を先に出さない() {
    let engine = assemble(&RuntimeConfig {
        audio: Backend::Dsp,
        ..RuntimeConfig::sample()
    })
    .unwrap();
    let mut lane = MeterLane::default();
    lane.record(
        engine.mixers().as_ref(),
        0.0,
        &tone(0.5, 4_800),
        level("a", 0.5),
    );
    lane.record(
        engine.mixers().as_ref(),
        0.1,
        &tone(0.1, 4_800),
        level("a", 0.0),
    );
    let (tracks, master) = lane.at(0.05).unwrap();
    let db = tracks[0].peak_db.unwrap();
    assert!((db - 20.0 * 0.5f64.log10()).abs() < 1e-6);
    assert_eq!(master.peak_db.len(), 2);
    let (tracks, _) = lane.at(0.15).unwrap();
    assert_eq!(tracks[0].peak_db, None, "無音は None");
    lane.reset();
    assert!(lane.at(0.15).is_none());
    assert!(MeterLane::default().at(0.0).is_none());
}

#[test]
fn ラウドネスの読みを_db_にし_無音は_none() {
    let v = master_view(&MasterReading {
        peak: vec![1.0, 0.0],
        true_peak: Some(0.5),
        short_term_lufs: Some(-14.0),
        integrated_lufs: None,
    });
    assert_eq!(v.peak_db, vec![Some(0.0), None]);
    assert!((v.true_peak_db.unwrap() - -6.0206).abs() < 1e-3);
    assert_eq!(v.short_term_lufs, Some(-14.0));
    assert_eq!(v.integrated_lufs, None);
}
