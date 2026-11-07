//! ラウドネスのメーター。EBU R128 / ITU-R BS.1770 の計算は `ebur128` に任せる。

use ebur128::{EbuR128, Mode};

use mm_audio_api::{AudioBlock, LoudnessMeter, MasterMeter, MasterReading};
use mm_types::{MmError, Result};

/// 統合ラウドネスに要る最短の長さ（秒）。BS.1770 の 1 ブロック（400 ms）に合わせる。
const MIN_SECONDS: f64 = 0.4;
/// 短期ラウドネスの窓（秒）。BS.1770 の短期の定義。`ebur128` は埋まるまでの窓を 0 で埋めて平均するので、
/// 再生の始めとシークの後に低く出る値を出さない。
const SHORT_TERM_SECONDS: f64 = 3.0;

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

/// Mixer のマスターのメーター。短期・統合ラウドネスとトゥルーピークを `ebur128` で測る。
pub struct DspMasterMeter {
    sample_rate: u32,
    channels: u16,
    state: EbuR128,
    frames: u64,
}

impl DspMasterMeter {
    /// 形を決めて作る。0 ch、0 Hz は `InvalidArgument`。
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self> {
        let mode = Mode::S | Mode::I | Mode::SAMPLE_PEAK | Mode::TRUE_PEAK;
        let state = EbuR128::new(u32::from(channels), sample_rate, mode)
            .map_err(|e| MmError::InvalidArgument(format!("メーターを作れない: {e}")))?;
        Ok(Self {
            sample_rate,
            channels,
            state,
            frames: 0,
        })
    }

    fn has_seconds(&self, seconds: f64) -> bool {
        self.frames as f64 >= seconds * f64::from(self.sample_rate)
    }

    /// チャンネルの値を `get` で集める。読めないチャンネルは 0。
    fn per_channel(&self, get: impl Fn(&EbuR128, u32) -> f64) -> Vec<f32> {
        (0..u32::from(self.channels))
            .map(|c| get(&self.state, c) as f32)
            .collect()
    }
}

/// 有限の値だけ `Some`。`ebur128` は無音で -inf を返す。
fn finite(v: std::result::Result<f64, ebur128::Error>) -> Option<f32> {
    v.ok().filter(|v| v.is_finite()).map(|v| v as f32)
}

impl MasterMeter for DspMasterMeter {
    fn feed(&mut self, block: &AudioBlock) -> Result<MasterReading> {
        if block.sample_rate != self.sample_rate || block.channels != self.channels {
            return Err(MmError::InvalidArgument("メーターと入力の形が違う".into()));
        }
        self.state
            .add_frames_f32(&block.samples)
            .map_err(|e| MmError::Internal(format!("ラウドネスの計測に失敗: {e}")))?;
        self.frames += block.frames() as u64;
        let peak = self.per_channel(|s, c| s.prev_sample_peak(c).unwrap_or(0.0));
        let true_peak = self
            .per_channel(|s, c| s.true_peak(c).unwrap_or(0.0))
            .into_iter()
            .fold(0.0f32, f32::max);
        let short_full = self.has_seconds(SHORT_TERM_SECONDS);
        let enough = self.has_seconds(MIN_SECONDS);
        Ok(MasterReading {
            peak,
            true_peak: (true_peak > 0.0).then_some(true_peak),
            short_term_lufs: finite(self.state.loudness_shortterm()).filter(|_| short_full),
            integrated_lufs: finite(self.state.loudness_global()).filter(|_| enough),
        })
    }

    fn reset(&mut self) {
        self.state.reset();
        self.frames = 0;
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
