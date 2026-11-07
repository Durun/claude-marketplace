//! Mixer の設定（音量、ミュート、ダッキング）が、プレビューと同じ音で書き出しに効く。
//! レンダラーは wgpu（区間をまたいでミキサーの状態を持ち越す本物の経路）。GPU が無い環境ではスキップし、
//! `MM_REQUIRE_GPU` があれば失敗にする。

use super::*;
use mm_audio_api::AudioBlock;
use mm_decode_api::{EncodeReport, EncodeSettings, Encoder};
use mm_doc::{CompMixer, Ducking, Layer, MixerBus};
use mm_runtime::{assemble, Backend, Engine, RuntimeConfig};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::vertical_project;
use mm_types::{CompositionId, Id, RationalTime, RgbaImage};

/// プレビューが 1 回に混ぜるフレーム数（アプリの `playback/player.rs` の `AUDIO_CHUNK` と同じ 0.1 秒）。
const PREVIEW_CHUNK_FRAMES: i64 = 4_800;
/// コンポジションの長さ（秒）。
const COMP_SECONDS: i64 = 3;
/// 声のレイヤーが鳴る区間（秒）。
const VOICE_FROM: i64 = 1;
const VOICE_TO: i64 = 2;
/// BGM のバスの音量（dB）。
const BGM_BUS_DB: f64 = -3.0;

/// 映像は捨て、音のフレーム数だけ数えるエンコーダー。
struct Discard;

impl Encoder for Discard {
    fn begin(&mut self, _settings: &EncodeSettings) -> Result<()> {
        Ok(())
    }

    fn push_frames(&mut self, _frames: &[RgbaImage]) -> Result<()> {
        Ok(())
    }

    fn push_audio(&mut self, _block: &AudioBlock) -> Result<()> {
        Ok(())
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        Err(MmError::Internal("このテストでは閉じない".into()))
    }
}

fn wgpu_engine() -> Option<Engine> {
    let config = RuntimeConfig {
        render: Backend::Wgpu,
        audio: Backend::Dsp,
        ..RuntimeConfig::sample()
    };
    match assemble(&config) {
        Ok(e) => Some(e),
        Err(MmError::Unsupported(e)) if std::env::var_os("MM_REQUIRE_GPU").is_none() => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
        Err(e) => panic!("組めない: {e}"),
    }
}

fn secs(s: i64) -> RationalTime {
    RationalTime::seconds(s)
}

/// 声（1〜2 秒）と BGM（全体）と、ミュートした効果音（全体）。BGM のバスは -3 dB、声で BGM を下げる。
fn mixer_project(ducking: bool, mute_voice: bool) -> (Project, CompositionId) {
    let (mut project, comp_id, _) =
        vertical_project(&SampleMedia::default(), Path::new("unused.json"));
    let comp = project.composition_mut(&comp_id).unwrap();
    comp.duration = secs(COMP_SECONDS);
    comp.layers.retain(|l| l.id.as_str() == "l-footage");
    let voice = comp.layers.pop().unwrap();
    let with = |id: &str, role: &str, start: i64, len: i64, mute: bool| -> Layer {
        let mut l = voice.clone();
        l.id = Id::new(id);
        l.start = secs(start);
        l.out_point = l.in_point.checked_add(secs(len)).unwrap();
        let audio = l.audio.as_mut().unwrap();
        audio.role = role.into();
        audio.mute = mute;
        l
    };
    comp.layers = vec![
        with("bgm", "bgm", 0, COMP_SECONDS, false),
        with("sfx", "sfx", 0, COMP_SECONDS, true),
        with(
            "voice",
            "voice",
            VOICE_FROM,
            VOICE_TO - VOICE_FROM,
            mute_voice,
        ),
    ];
    comp.mixer = Some(CompMixer {
        master_db: 0.0,
        buses: [(
            "bgm".to_owned(),
            MixerBus {
                gain_db: BGM_BUS_DB,
            },
        )]
        .into(),
        ducking: ducking.then(Ducking::voice_over_bgm),
    });
    (project, comp_id)
}

/// 書き出しと同じ経路（`render_video_and_audio`）で混ぜた音。
fn exported(engine: &mut Engine, project: &Project, comp: &CompositionId) -> AudioBlock {
    let media = SampleMedia::default();
    let settings = EncodeSettings {
        size: media.size,
        fps: media.fps,
        codec: "sample".into(),
        output_path: PathBuf::from("unused.json"),
        audio: None,
    };
    let range = project.composition(comp).unwrap().range().unwrap();
    let control = ExportControl::default();
    let plan = Plan {
        target: &Id::new("mp4"),
        composition: comp,
        range,
        control: &control,
        parallel: 0,
        parallel_min_frames: None,
        pass: PassPosition { index: 1, of: 1 },
    };
    passes::render_video_and_audio(engine, project, &plan, &settings, range, &mut Discard)
        .unwrap()
        .unwrap()
        .1
}

/// プレビューと同じく 0.1 秒ずつ混ぜてつないだ音。
fn previewed(engine: &mut Engine, project: &Project, comp: &CompositionId) -> AudioBlock {
    let rate = i128::from(mm_audio_api::DEFAULT_SAMPLE_RATE);
    let total = COMP_SECONDS * rate as i64;
    let at = |n: i64| RationalTime::new(i128::from(n), rate as i64 as u32).unwrap();
    let mut all: Option<AudioBlock> = None;
    let mut start = 0;
    while start < total {
        let end = (start + PREVIEW_CHUNK_FRAMES).min(total);
        let range = TimeRange::new(at(start), at(end)).unwrap();
        let block = engine.render_audio(project, comp, range).unwrap();
        match &mut all {
            Some(a) => a.append(&block).unwrap(),
            None => all = Some(block),
        }
        start = end;
    }
    all.unwrap()
}

/// `[from, to)` 秒の左チャンネルのピーク。
fn peak(block: &AudioBlock, from: f64, to: f64) -> f32 {
    let rate = f64::from(block.sample_rate);
    let ch = block.channels as usize;
    let (a, b) = ((from * rate) as usize, (to * rate) as usize);
    block.samples[a * ch..b * ch]
        .iter()
        .step_by(ch)
        .fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn ダッキングと音量とミュートはプレビューと同じ音で書き出しに効く() {
    let Some(mut engine) = wgpu_engine() else {
        return;
    };
    let (project, comp) = mixer_project(true, false);
    let export = exported(&mut engine, &project, &comp);
    let mut engine = wgpu_engine().unwrap();
    let preview = previewed(&mut engine, &project, &comp);
    assert_eq!(export.samples.len(), preview.samples.len());
    assert_eq!(
        export.max_diff(&preview),
        Some(0.0),
        "プレビューと書き出しが bit で一致"
    );

    let (plain, _) = mixer_project(false, false);
    let undocked = exported(&mut wgpu_engine().unwrap(), &plain, &comp);
    // 声の間（立ち上がりの 20 ms を避けた所）は BGM が下がり、声が終わって戻りきった後は同じ。
    assert!(peak(&export, 1.1, 1.9) < peak(&undocked, 1.1, 1.9) * 0.8);
    assert_eq!(peak(&export, 2.5, 3.0), peak(&undocked, 2.5, 3.0));

    // BGM だけの所は、素材の振幅に BGM のバスの音量をかけた大きさ（ミュートの効果音は混ざらない）。
    let bgm_only = mm_testkit::decode::TONE_AMPLITUDE * 10f32.powf(BGM_BUS_DB as f32 / 20.0);
    assert!((peak(&export, 0.2, 0.9) - bgm_only).abs() < 1e-3);
    let (muted, _) = mixer_project(true, true);
    let muted = exported(&mut wgpu_engine().unwrap(), &muted, &comp);
    assert!(
        (peak(&muted, 1.1, 1.9) - bgm_only).abs() < 1e-3,
        "声をミュートするとダッキングもしない"
    );
}

#[test]
fn トラックのメーターは鳴っているレイヤーの音量の後のピーク() {
    let Some(mut engine) = wgpu_engine() else {
        return;
    };
    let (project, comp) = mixer_project(false, false);
    let range = TimeRange::new(secs(VOICE_FROM), RationalTime::new(3, 2).unwrap()).unwrap();
    engine.render_audio(&project, &comp, range).unwrap();
    let levels = engine.audio_levels();
    let ids: Vec<&str> = levels.iter().map(|l| l.layer.as_str()).collect();
    assert_eq!(ids, ["bgm", "voice"], "ミュートの効果音は入らない");
    let tone = mm_testkit::decode::TONE_AMPLITUDE;
    assert!(
        levels.iter().all(|l| (l.peak - tone).abs() < 1e-3),
        "{levels:?}"
    );
}
