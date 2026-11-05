//! ラウドネスのメーター。EBU R128 / ITU-R BS.1770 の計算は `ebur128` に任せる。

use ebur128::{EbuR128, Mode};

use mm_audio_api::{AudioBlock, LoudnessMeter};
use mm_types::{MmError, Result};

/// 統合ラウドネスに要る最短の長さ（秒）。BS.1770 の 1 ブロック（400 ms）に合わせる。
const MIN_SECONDS: f64 = 0.4;

/// BS.1770 のメーター。
pub struct DspLoudnessMeter {
    sample_rate: u32,
    channels: u16,
    state: EbuR128,
    frames: u64,
}

impl DspLoudnessMeter {
    /// 形を決めて作る。0 ch、0 Hz は `InvalidArgument`。
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self> {
        let state = EbuR128::new(u32::from(channels), sample_rate, Mode::I)
            .map_err(|e| MmError::InvalidArgument(format!("メーターを作れない: {e}")))?;
        Ok(Self {
            sample_rate,
            channels,
            state,
            frames: 0,
        })
    }

    /// 直近 400 ms の瞬間ラウドネス（LUFS）。無音や短すぎる入力は `None`。
    pub fn momentary_lufs(&self) -> Option<f32> {
        let v = self.state.loudness_momentary().ok()?;
        (self.enough_frames() && v.is_finite()).then_some(v as f32)
    }

    fn enough_frames(&self) -> bool {
        self.frames as f64 >= MIN_SECONDS * f64::from(self.sample_rate)
    }
}

impl LoudnessMeter for DspLoudnessMeter {
    fn feed(&mut self, block: &AudioBlock) -> Result<()> {
        if block.sample_rate != self.sample_rate || block.channels != self.channels {
            return Err(MmError::InvalidArgument("メーターと入力の形が違う".into()));
        }
        self.state
            .add_frames_f32(&block.samples)
            .map_err(|e| MmError::Internal(format!("ラウドネスの計測に失敗: {e}")))?;
        self.frames += block.frames() as u64;
        Ok(())
    }

    fn integrated_lufs(&self) -> Option<f32> {
        if !self.enough_frames() {
            return None;
        }
        let v = self.state.loudness_global().ok()?;
        v.is_finite().then_some(v as f32)
    }

    fn reset(&mut self) {
        self.state.reset();
        self.frames = 0;
    }
}
