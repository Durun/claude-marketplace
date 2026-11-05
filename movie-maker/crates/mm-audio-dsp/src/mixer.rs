//! ミキサー。入力を役割ごとのバスに足し、バスの音量とダッキングをかけてマスターに足す。
//!
//! 状態を持つのはダッキングの滑らかな音量だけで、同じミキサーに同じ入力を同じ順に渡せば同じ出力になる。

use std::collections::BTreeMap;

use mm_audio_api::{AudioBlock, MixInput, Mixer, MixerConfig, MixerFactory};
use mm_types::{MmError, Result};

/// dB を倍率に。
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// ミキサーを作る口。設定は `create` で受ける。
#[derive(Debug, Clone, Copy, Default)]
pub struct DspMixerFactory;

impl MixerFactory for DspMixerFactory {
    fn id(&self) -> &'static str {
        "dsp"
    }

    fn create(
        &self,
        sample_rate: u32,
        channels: u16,
        config: &MixerConfig,
    ) -> Result<Box<dyn Mixer>> {
        if sample_rate == 0 || channels == 0 {
            return Err(MmError::InvalidArgument(
                "サンプルレートとチャンネル数は 1 以上".into(),
            ));
        }
        Ok(Box::new(DspMixer {
            sample_rate,
            channels,
            config: config.clone(),
            duck_gain: 1.0,
        }))
    }
}

/// 自前 DSP のミキサー。
#[derive(Debug)]
pub struct DspMixer {
    sample_rate: u32,
    channels: u16,
    config: MixerConfig,
    /// ダッキング中の対象バスにかかっている、滑らかにした倍率（1.0 で下げていない）。
    duck_gain: f32,
}

/// 中央で両方 1.0 になるパン（バランス型）。中央の素通しを保つため等電力則は使わない。
fn pan_gains(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    ((1.0 - p).min(1.0), (1.0 + p).min(1.0))
}

/// 時間（ms）で 63% 動く 1 次の係数。0 ms は即時。
fn smoothing(ms: f32, sample_rate: u32) -> f32 {
    if ms <= 0.0 {
        return 1.0;
    }
    1.0 - (-1000.0 / (ms * sample_rate as f32)).exp()
}

impl DspMixer {
    fn check(&self, block: &AudioBlock) -> Result<()> {
        if block.channels != self.channels || block.sample_rate != self.sample_rate {
            return Err(MmError::InvalidArgument("入力の形がミキサーと違う".into()));
        }
        Ok(())
    }

    /// 入力を役割ごとのバスに足す。役割の並びは `BTreeMap` で固定し、足す順を決定的にする。
    fn sum_buses<'a>(
        &self,
        inputs: &'a [MixInput],
        frames: usize,
    ) -> Result<BTreeMap<&'a str, Vec<f32>>> {
        let ch = self.channels as usize;
        let mut buses: BTreeMap<&str, Vec<f32>> = BTreeMap::new();
        for input in inputs {
            self.check(&input.block)?;
            let bus = buses
                .entry(input.role.as_str())
                .or_insert_with(|| vec![0.0; frames * ch]);
            let gain = db_to_gain(input.gain_db);
            let (l, r) = pan_gains(input.pan);
            let n = input.block.frames().min(frames);
            for i in 0..n {
                for c in 0..ch {
                    let side = match (ch, c) {
                        (2, 0) => l,
                        (2, 1) => r,
                        _ => 1.0,
                    };
                    bus[i * ch + c] += input.block.samples[i * ch + c] * gain * side;
                }
            }
        }
        Ok(buses)
    }

    /// 対象バスに、トリガーのバスの鳴り方に追従する倍率をかける。
    fn apply_ducking(&mut self, buses: &mut BTreeMap<&str, Vec<f32>>, frames: usize) {
        let Some(duck) = self.config.ducking.clone() else {
            return;
        };
        let ch = self.channels as usize;
        let floor = db_to_gain(duck.reduction_db);
        let attack = smoothing(duck.attack_ms, self.sample_rate);
        let release = smoothing(duck.release_ms, self.sample_rate);
        let trigger = buses.get(duck.trigger.as_str()).cloned();
        let mut gain = self.duck_gain;
        for i in 0..frames {
            let level = trigger.as_ref().map_or(0.0, |t| {
                t[i * ch..(i + 1) * ch]
                    .iter()
                    .fold(0.0f32, |m, s| m.max(s.abs()))
            });
            let (goal, k) = if level > duck.threshold {
                (floor, attack)
            } else {
                (1.0, release)
            };
            gain += (goal - gain) * k;
            for role in &duck.targets {
                if let Some(bus) = buses.get_mut(role.as_str()) {
                    for s in &mut bus[i * ch..(i + 1) * ch] {
                        *s *= gain;
                    }
                }
            }
        }
        self.duck_gain = gain;
    }
}

impl Mixer for DspMixer {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn mix(&mut self, inputs: &[MixInput], frames: usize) -> Result<AudioBlock> {
        let mut buses = self.sum_buses(inputs, frames)?;
        self.apply_ducking(&mut buses, frames);
        let master = db_to_gain(self.config.master_db);
        let mut out = vec![0.0f32; frames * self.channels as usize];
        for (role, bus) in &buses {
            let gain = self
                .config
                .buses
                .get(*role)
                .map_or(1.0, |b| db_to_gain(b.gain_db));
            for (o, s) in out.iter_mut().zip(bus) {
                *o += s * gain;
            }
        }
        if master != 1.0 {
            out.iter_mut().for_each(|s| *s *= master);
        }
        AudioBlock::from_samples(self.sample_rate, self.channels, out)
    }
}
