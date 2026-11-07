//! 音量、パン、バス、ダッキング、ラウドネスの振る舞い。

use std::collections::BTreeMap;

use mm_audio_api::{
    AudioBlock, BusConfig, DuckingConfig, LoudnessMeter, MixInput, Mixer, MixerConfig, MixerFactory,
};
use mm_audio_dsp::{db_to_gain, DspLoudnessMeter, DspMixerFactory};

const RATE: u32 = 48_000;

fn sine(freq: f32, amp: f32, frames: usize) -> AudioBlock {
    let mut s = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let v = amp * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin();
        s.extend([v, v]);
    }
    AudioBlock::from_samples(RATE, 2, s).expect("形は合う")
}

fn input(block: AudioBlock, role: &str, gain_db: f32, pan: f32) -> MixInput {
    MixInput {
        block,
        role: role.into(),
        gain_db,
        pan,
    }
}

fn mixer(config: MixerConfig) -> Box<dyn Mixer> {
    DspMixerFactory.create(RATE, 2, &config).expect("作れる")
}

fn peak(b: &AudioBlock, channel: usize) -> f32 {
    b.samples
        .iter()
        .skip(channel)
        .step_by(2)
        .fold(0.0, |m, s| m.max(s.abs()))
}

#[test]
fn 音量は_db_で掛かる() {
    let mut m = mixer(MixerConfig::default());
    let out = m
        .mix(&[input(sine(440.0, 0.5, 4_800), "voice", -6.0, 0.0)], 4_800)
        .expect("混ぜる");
    let expect = 0.5 * db_to_gain(-6.0);
    assert!((peak(&out, 0) - expect).abs() < 1e-3);
}

#[test]
fn パンで反対側が小さくなる() {
    let mut m = mixer(MixerConfig::default());
    let out = m
        .mix(&[input(sine(440.0, 0.5, 4_800), "sfx", 0.0, 1.0)], 4_800)
        .expect("混ぜる");
    assert_eq!(peak(&out, 0), 0.0, "右いっぱいで左は無音");
    assert!(peak(&out, 1) > 0.49);
}

#[test]
fn バスの音量とマスターが掛かる() {
    let config = MixerConfig {
        buses: BTreeMap::from([("bgm".into(), BusConfig { gain_db: -20.0 })]),
        master_db: -6.0,
        ..MixerConfig::default()
    };
    let mut m = mixer(config);
    let out = m
        .mix(&[input(sine(440.0, 0.5, 4_800), "bgm", 0.0, 0.0)], 4_800)
        .expect("混ぜる");
    let expect = 0.5 * db_to_gain(-26.0);
    assert!((peak(&out, 0) - expect).abs() < 1e-3);
}

#[test]
fn 声が鳴ると_bgm_が下がり止むと戻る() {
    let config = MixerConfig {
        ducking: Some(DuckingConfig::voice_over_bgm()),
        ..MixerConfig::default()
    };
    let mut m = mixer(config);
    let frames = RATE as usize; // 1 秒
    let bgm = input(sine(220.0, 0.5, frames * 2), "bgm", 0.0, 0.0);
    let mut voice_block = AudioBlock::silence(RATE, 2, frames * 2);
    voice_block.samples[..frames * 2].copy_from_slice(&sine(440.0, 0.5, frames).samples);
    let voice = input(voice_block, "voice", 0.0, 0.0);

    let ducked = m.mix(&[bgm.clone(), voice], frames).expect("混ぜる");
    let alone = {
        let mut m2 = mixer(MixerConfig::default());
        m2.mix(&[bgm], frames).expect("混ぜる")
    };
    // 声の有無で BGM 分だけの差が出ることを、最後の 0.1 秒の BGM の大きさで見る。
    let tail = frames - 4_800;
    let tail_peak = |b: &AudioBlock| {
        b.samples[tail * 2..]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()))
    };
    assert!(
        tail_peak(&ducked) > tail_peak(&alone),
        "声の分が足されている"
    );

    // 声が止んだ次のブロックでは、BGM は release の時間をかけて 0 dB に戻る。
    let silent = input(AudioBlock::silence(RATE, 2, frames), "voice", 0.0, 0.0);
    let bgm2 = input(sine(220.0, 0.5, frames), "bgm", 0.0, 0.0);
    let after = m.mix(&[bgm2, silent], frames).expect("混ぜる");
    let head = peak(
        &AudioBlock::from_samples(RATE, 2, after.samples[..960].to_vec()).unwrap(),
        0,
    );
    let end = after.samples[(frames - 960) * 2..]
        .iter()
        .fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(head < 0.2, "止んだ直後はまだ下がっている（{head}）");
    assert!(end > 0.45, "1 秒後には戻っている（{end}）");
}

/// 一定の値の入力。倍率をそのまま読み取れる。
fn constant(value: f32, frames: usize) -> AudioBlock {
    AudioBlock::from_samples(RATE, 2, vec![value; frames * 2]).expect("形は合う")
}

/// 声（`voice`）と BGM（一定の 1.0）を混ぜて、BGM にかかった倍率の列を返す。
fn duck_gains(voice: AudioBlock, config: DuckingConfig) -> Vec<f32> {
    let frames = voice.frames();
    let voice_left: Vec<f32> = voice.samples.iter().step_by(2).copied().collect();
    let mut m = mixer(MixerConfig {
        ducking: Some(config),
        ..MixerConfig::default()
    });
    let out = m
        .mix(
            &[
                input(constant(1.0, frames), "bgm", 0.0, 0.0),
                input(voice, "voice", 0.0, 0.0),
            ],
            frames,
        )
        .expect("混ぜる");
    out.samples
        .iter()
        .step_by(2)
        .zip(&voice_left)
        .map(|(o, v)| o - v)
        .collect()
}

#[test]
fn ダッキングの検出はゼロ交差で解除されない() {
    // しきい値 0.01 のすぐ上の低い声。波形が 0 を通る間も鳴っている扱いで、倍率は一定に保たれる。
    let frames = RATE as usize / 2;
    let gains = duck_gains(sine(80.0, 0.012, frames), DuckingConfig::voice_over_bgm());
    let (lo, hi) = gains[frames / 2..]
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), g| (lo.min(*g), hi.max(*g)));
    let floor = db_to_gain(-12.0);
    assert!(
        (lo - floor).abs() < 1e-3 && (hi - floor).abs() < 1e-3,
        "{lo} {hi}"
    );
}

#[test]
fn アタックは下がりきるまでの時間() {
    let config = DuckingConfig::voice_over_bgm(); // attack 20 ms、-12 dB
    let floor = db_to_gain(-12.0);
    let frames = RATE as usize / 10;
    let gains = duck_gains(constant(0.5, frames), config);
    let bgm = |i: usize| gains[i];
    let half = (1.0 + floor) / 2.0;
    assert!(
        (bgm(480) - half).abs() < 0.02,
        "10 ms で半分（{}）",
        bgm(480)
    );
    assert!((bgm(960 + 10) - floor).abs() < 0.01, "20 ms で下がりきる");
}

#[test]
fn 区間を分けて混ぜても一度に混ぜた結果と同じ() {
    let config = MixerConfig {
        ducking: Some(DuckingConfig::voice_over_bgm()),
        ..MixerConfig::default()
    };
    let frames = 9_600;
    let mut voice = AudioBlock::silence(RATE, 2, frames);
    voice.samples[4_000..]
        .copy_from_slice(&constant(0.5, frames - 2_000).samples[..frames * 2 - 4_000]);
    let bgm = constant(0.5, frames);
    let whole = mixer(config.clone())
        .mix(
            &[
                input(bgm.clone(), "bgm", 0.0, 0.0),
                input(voice.clone(), "voice", 0.0, 0.0),
            ],
            frames,
        )
        .expect("混ぜる");
    let mut m = mixer(config);
    let mut pieces = AudioBlock::silence(RATE, 2, 0);
    for start in (0..frames).step_by(1_200) {
        let part = |b: &AudioBlock, role: &str| {
            let s = b.samples[start * 2..(start + 1_200) * 2].to_vec();
            input(
                AudioBlock::from_samples(RATE, 2, s).unwrap(),
                role,
                0.0,
                0.0,
            )
        };
        let out = m
            .mix(&[part(&bgm, "bgm"), part(&voice, "voice")], 1_200)
            .expect("混ぜる");
        pieces.append(&out).expect("つなげる");
    }
    assert_eq!(whole.max_diff(&pieces), Some(0.0));
}

#[test]
fn ダッキングは同じ入力で同じ出力() {
    let config = MixerConfig {
        ducking: Some(DuckingConfig::voice_over_bgm()),
        ..MixerConfig::default()
    };
    let inputs = [
        input(sine(220.0, 0.4, 9_600), "bgm", 0.0, -0.3),
        input(sine(440.0, 0.4, 9_600), "voice", -3.0, 0.2),
    ];
    let a = mixer(config.clone()).mix(&inputs, 9_600).expect("混ぜる");
    let b = mixer(config).mix(&inputs, 9_600).expect("混ぜる");
    assert_eq!(a, b);
}

#[test]
fn 形の違う入力は拒否する() {
    let mut m = mixer(MixerConfig::default());
    let wrong = AudioBlock::silence(44_100, 2, 100);
    assert!(m.mix(&[input(wrong, "voice", 0.0, 0.0)], 100).is_err());
}

#[test]
fn ラウドネスは基準の正弦波で規格どおりの値になる() {
    // 997 Hz、振幅 0.1（-20 dBFS）の同相ステレオは、正弦波の RMS -3 dB と 2 ch 加算 +3 dB が打ち消し合い -20 LUFS になる。
    let mut meter = DspLoudnessMeter::new(RATE, 2).expect("作れる");
    assert_eq!(meter.integrated_lufs(), None, "未入力は None");
    meter
        .feed(&sine(997.0, 0.1, RATE as usize * 3))
        .expect("食わせる");
    let lufs = meter.integrated_lufs().expect("3 秒あれば測れる");
    assert!((lufs - -20.0).abs() < 0.1, "{lufs}");
    meter.reset();
    assert_eq!(meter.integrated_lufs(), None);
}

#[test]
fn 短すぎる入力と無音は_none() {
    let mut meter = DspLoudnessMeter::new(RATE, 2).expect("作れる");
    meter.feed(&sine(997.0, 0.1, 4_800)).expect("食わせる");
    assert_eq!(meter.integrated_lufs(), None);
    meter.reset();
    meter
        .feed(&AudioBlock::silence(RATE, 2, RATE as usize))
        .expect("食わせる");
    assert_eq!(meter.integrated_lufs(), None);
    assert!(meter.feed(&AudioBlock::silence(44_100, 2, 10)).is_err());
}
