//! 音の見本と契約テスト。見本のミキサーは音量とパンを掛けて足すだけ。無音は無音のまま。

use std::sync::Arc;

use mm_audio_api::{AudioBlock, AudioSink, MixInput, Mixer, MixerFactory};
use mm_types::{MmError, Result};

/// 素通しの判定に使う許容差。
pub const PASSTHROUGH_TOLERANCE: f32 = 1e-6;

/// dB を倍率に。
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// 見本のミキサーを作る口。
#[derive(Debug, Clone, Default)]
pub struct SampleMixerFactory;

impl MixerFactory for SampleMixerFactory {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn create(&self, sample_rate: u32, channels: u16) -> Result<Box<dyn Mixer>> {
        Ok(Box::new(SampleMixer {
            sample_rate,
            channels,
        }))
    }
}

/// 見本のミキサー。
#[derive(Debug)]
pub struct SampleMixer {
    sample_rate: u32,
    channels: u16,
}

impl Mixer for SampleMixer {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn mix(&mut self, inputs: &[MixInput], frames: usize) -> Result<AudioBlock> {
        let ch = self.channels as usize;
        let mut out = vec![0.0f32; frames * ch];
        for input in inputs {
            if input.block.channels != self.channels || input.block.sample_rate != self.sample_rate
            {
                return Err(MmError::InvalidArgument("入力の形がミキサーと違う".into()));
            }
            let gain = db_to_gain(input.gain_db);
            let (l, r) = pan_gains(input.pan);
            let n = input.block.frames().min(frames);
            for i in 0..n {
                for c in 0..ch {
                    let side = if ch == 2 && c == 1 { r } else { l };
                    out[i * ch + c] += input.block.samples[i * ch + c] * gain * side;
                }
            }
        }
        AudioBlock::from_samples(self.sample_rate, self.channels, out)
    }
}

/// 中央で両方 1.0 の、素通しを保つパン。
fn pan_gains(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    ((1.0 - p).min(1.0), (1.0 + p).min(1.0))
}

/// 受け取ったブロックをためる出口。
#[derive(Debug, Default)]
pub struct VecSink {
    pub sample_rate: u32,
    pub channels: u16,
    pub blocks: Vec<AudioBlock>,
}

impl VecSink {
    /// 形を決めて作る。
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            blocks: Vec::new(),
        }
    }
}

impl AudioSink for VecSink {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn write(&mut self, block: &AudioBlock) -> Result<()> {
        if block.sample_rate != self.sample_rate || block.channels != self.channels {
            return Err(MmError::InvalidArgument("出口の形と違う".into()));
        }
        self.blocks.push(block.clone());
        Ok(())
    }

    fn finish(&mut self) -> Result<u64> {
        Ok(self.blocks.iter().map(|b| b.frames() as u64).sum())
    }
}

fn tone(rate: u32, channels: u16, frames: usize) -> AudioBlock {
    let mut samples = Vec::with_capacity(frames * channels as usize);
    for i in 0..frames {
        let v = ((i % 97) as f32 / 97.0) - 0.5;
        samples.extend(std::iter::repeat_n(v, channels as usize));
    }
    AudioBlock::from_samples(rate, channels, samples).expect("形は合う")
}

/// ミキサーの契約。
pub fn contract(make: impl Fn() -> Arc<dyn MixerFactory>) {
    let rate = mm_audio_api::DEFAULT_SAMPLE_RATE;
    let ch = mm_audio_api::DEFAULT_CHANNELS;
    let frames = 4_800;
    let factory = make();
    let mut mixer = factory.create(rate, ch).expect("作れる");
    assert_eq!((mixer.sample_rate(), mixer.channels()), (rate, ch));

    let silence = MixInput {
        block: AudioBlock::silence(rate, ch, frames),
        role: "voice".into(),
        gain_db: 0.0,
        pan: 0.0,
    };
    let out = mixer
        .mix(std::slice::from_ref(&silence), frames)
        .expect("混ぜる");
    assert_eq!(out.frames(), frames, "頼んだコマ数");
    assert!(out.is_silent(), "無音は無音");

    let tone = MixInput {
        block: tone(rate, ch, frames),
        ..silence
    };
    let a = mixer
        .mix(std::slice::from_ref(&tone), frames)
        .expect("混ぜる");
    let b = mixer
        .mix(std::slice::from_ref(&tone), frames)
        .expect("混ぜる");
    assert_eq!(a, b, "決定性");
    let diff = a.max_diff(&tone.block).expect("同じ形");
    assert!(
        diff <= PASSTHROUGH_TOLERANCE,
        "0 dB・中央は素通し（差 {diff}）"
    );

    let short = MixInput {
        block: AudioBlock::silence(rate, ch, frames / 2),
        ..tone
    };
    let out = mixer.mix(&[short], frames).expect("混ぜる");
    assert_eq!(out.frames(), frames, "短い入力は無音で埋める");
}
