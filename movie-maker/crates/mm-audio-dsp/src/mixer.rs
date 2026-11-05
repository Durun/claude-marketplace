//! ミキサー。入力を役割ごとのバスに足し、バスの音量とダッキングをかけてマスターに足す。
//!
//! 状態を持つのはダッキングの検出と滑らかな音量だけで、同じミキサーに同じ入力を同じ順に渡せば同じ出力になる。
//! 区間を分けて渡しても、ブロックの切れ目で状態が戻らない。

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
            peak: 0.0,
            hold_left: 0,
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
    /// トリガーの振幅の保持した最大値。ゼロ交差で鳴っていないと見なさないための検出値。
    peak: f32,
    /// `peak` を保持する残りのコマ数。
    hold_left: usize,
}

/// トリガーの検出値を保持する時間（ms）。50 Hz より低い音の半周期より長く、ゼロ交差をまたぐ。
const DUCK_HOLD_MS: f32 = 10.0;

/// 中央で両方 1.0 になるパン（バランス型）。中央の素通しを保つため等電力則は使わない。
fn pan_gains(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    ((1.0 - p).min(1.0), (1.0 + p).min(1.0))
}

/// 1 コマで動かす 0..1 の全幅に対する割合。`ms` で動ききる。0 ms は即時。
fn slew_step(ms: f32, sample_rate: u32) -> f32 {
    if ms <= 0.0 {
        return f32::INFINITY;
    }
    1000.0 / (ms * sample_rate as f32)
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

    /// トリガーの検出値を 1 コマ進める。大きい値は即座に取り、小さい値は保持時間が過ぎるまで待つ。
    fn detect(&mut self, level: f32) -> f32 {
        let hold = (DUCK_HOLD_MS * self.sample_rate as f32 / 1000.0) as usize;
        if level >= self.peak || self.hold_left == 0 {
            self.peak = level;
            self.hold_left = hold;
        } else {
            self.hold_left -= 1;
        }
        self.peak
    }

    /// 対象バスに、トリガーのバスの鳴り方に追従する倍率をかける。
    /// 倍率は `attack_ms` / `release_ms` で 1.0 と下げた量の間を動ききる直線で動く。
    fn apply_ducking(&mut self, buses: &mut BTreeMap<&str, Vec<f32>>, frames: usize) {
        let Some(duck) = self.config.ducking.clone() else {
            return;
        };
        let ch = self.channels as usize;
        let floor = db_to_gain(duck.reduction_db);
        let travel = 1.0 - floor;
        let attack = slew_step(duck.attack_ms, self.sample_rate) * travel;
        let release = slew_step(duck.release_ms, self.sample_rate) * travel;
        let trigger = buses.get(duck.trigger.as_str()).cloned();
        for i in 0..frames {
            let level = trigger.as_ref().map_or(0.0, |t| {
                t[i * ch..(i + 1) * ch]
                    .iter()
                    .fold(0.0f32, |m, s| m.max(s.abs()))
            });
            let (goal, step) = if self.detect(level) > duck.threshold {
                (floor, attack)
            } else {
                (1.0, release)
            };
            let delta = goal - self.duck_gain;
            self.duck_gain += delta.clamp(-step, step);
            for role in &duck.targets {
                if let Some(bus) = buses.get_mut(role.as_str()) {
                    for s in &mut bus[i * ch..(i + 1) * ch] {
                        *s *= self.duck_gain;
                    }
                }
            }
        }
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
