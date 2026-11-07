//! Mixer のメーター。入力ごとのピーク（音量とパンの後）と、マスターのピーク・ラウドネス・トゥルーピーク。

use mm_audio_api::{AudioBlock, MixInput, Mixer, MixerConfig, MixerFactory};
use mm_audio_dsp::{db_to_gain, DspMixerFactory};

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

#[test]
fn 入力ごとのピークは音量の後で_入力の順に並ぶ() {
    let mut m: Box<dyn Mixer> = DspMixerFactory
        .create(RATE, 2, &MixerConfig::default())
        .expect("作れる");
    assert!(m.input_peaks().is_empty(), "混ぜる前は空");
    let inputs = [
        input(sine(440.0, 0.5, 4_800), "voice", -6.0, 0.0),
        input(AudioBlock::silence(RATE, 2, 4_800), "bgm", 0.0, 0.0),
    ];
    m.mix(&inputs, 4_800).expect("混ぜる");
    let peaks = m.input_peaks();
    assert_eq!(peaks.len(), 2);
    assert!(
        (peaks[0] - 0.5 * db_to_gain(-6.0)).abs() < 1e-3,
        "{peaks:?}"
    );
    assert_eq!(peaks[1], 0.0);
}

#[test]
fn マスターのメーターは基準の正弦波で_20_lufs_と振幅のピークになる() {
    let mut meter = DspMixerFactory.master_meter(RATE, 2).expect("dsp は持つ");
    let mut last = None;
    // 0.1 秒ずつ 3 秒。プレビューと同じく小さなブロックで食わせる。
    for _ in 0..30 {
        last = Some(meter.feed(&sine(997.0, 0.1, 4_800)).expect("食わせる"));
    }
    let r = last.expect("読んだ");
    assert_eq!(r.peak.len(), 2);
    assert!((r.peak[0] - 0.1).abs() < 1e-3, "{:?}", r.peak);
    let short = r.short_term_lufs.expect("3 秒あれば測れる");
    let integrated = r.integrated_lufs.expect("3 秒あれば測れる");
    assert!((short - -20.0).abs() < 0.2, "{short}");
    assert!((integrated - -20.0).abs() < 0.2, "{integrated}");
    let tp = r.true_peak.expect("鳴っている");
    assert!((0.1 - 1e-3..0.11).contains(&tp), "{tp}");
}

#[test]
fn 短期ラウドネスは_3_秒の窓が埋まるまで出さない() {
    let mut meter = DspMixerFactory.master_meter(RATE, 2).expect("dsp は持つ");
    let r = meter
        .feed(&sine(997.0, 0.1, RATE as usize))
        .expect("食わせる");
    assert_eq!(r.short_term_lufs, None, "1 秒では窓が埋まらない");
    assert!(r.integrated_lufs.is_some(), "統合は 400 ms から出す");
}

#[test]
fn マスターのメーターは無音で測れず_リセットで最初からになる() {
    let mut meter = DspMixerFactory.master_meter(RATE, 2).expect("dsp は持つ");
    let r = meter
        .feed(&AudioBlock::silence(RATE, 2, RATE as usize))
        .expect("食わせる");
    assert_eq!(r.integrated_lufs, None);
    assert_eq!(r.true_peak, None);
    assert_eq!(r.peak, vec![0.0, 0.0]);
    meter
        .feed(&sine(997.0, 0.5, RATE as usize))
        .expect("食わせる");
    meter.reset();
    let r = meter.feed(&sine(997.0, 0.1, 4_800)).expect("食わせる");
    assert_eq!(r.integrated_lufs, None, "リセットの後は短すぎて測れない");
    assert!(
        r.true_peak.expect("鳴っている") < 0.2,
        "前の大きい音を持ち越さない"
    );
    assert!(meter.feed(&AudioBlock::silence(44_100, 2, 10)).is_err());
}
