//! 音の口。ミキサーは Float32、サンプル単位、インターリーブで、プレビューと書き出しで同じ DSP を回す。
//!
//! 契約（`mm_testkit::audio::contract`）:
//! - 決定性: 同じ入力を 2 回混ぜると、サンプル列が bit で一致する。
//! - 無音の入力は無音を返す。
//! - 入力 1 本、0 dB、パン中央は素通し（許容差 1e-6）。
//! - 返すブロックの `frames()` は頼んだ `frames` と一致する。
//! - バスの音量（`buses[role].gain_db`）とマスター（`master_db`）は全実装が効かせる。ダッキングは実装が省いてよい（見本は省く）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use mm_types::{MmError, Result};

/// 既定のサンプルレート。
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// 既定のチャンネル数（ステレオ）。
pub const DEFAULT_CHANNELS: u16 = 2;

/// 音のブロック。`samples` はインターリーブ（L R L R ...）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioBlock {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl AudioBlock {
    /// `frames` フレーム分の無音。
    pub fn silence(sample_rate: u32, channels: u16, frames: usize) -> Self {
        Self {
            sample_rate,
            channels,
            samples: vec![0.0; frames * channels as usize],
        }
    }

    /// サンプルの列から。長さがチャンネル数で割り切れなければ `InvalidArgument`。
    pub fn from_samples(sample_rate: u32, channels: u16, samples: Vec<f32>) -> Result<Self> {
        if channels == 0 || !samples.len().is_multiple_of(channels as usize) {
            return Err(MmError::InvalidArgument(format!(
                "AudioBlock: {} サンプルは {channels} ch で割り切れない",
                samples.len()
            )));
        }
        Ok(Self {
            sample_rate,
            channels,
            samples,
        })
    }

    /// フレーム数（1 フレーム = 全チャンネルの 1 サンプル）。
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    /// 全サンプルが 0 か。
    pub fn is_silent(&self) -> bool {
        self.samples.iter().all(|s| *s == 0.0)
    }

    /// 2 つのブロックの、サンプルごとの差の最大。形が違えば `None`。
    pub fn max_diff(&self, other: &Self) -> Option<f32> {
        if self.sample_rate != other.sample_rate
            || self.channels != other.channels
            || self.samples.len() != other.samples.len()
        {
            return None;
        }
        Some(
            self.samples
                .iter()
                .zip(&other.samples)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max),
        )
    }

    /// 末尾に続ける。形が違えば `InvalidArgument`。
    pub fn append(&mut self, other: &Self) -> Result<()> {
        if self.sample_rate != other.sample_rate || self.channels != other.channels {
            return Err(MmError::InvalidArgument("AudioBlock: 形が違う".into()));
        }
        self.samples.extend_from_slice(&other.samples);
        Ok(())
    }
}

/// ミキサーへの入力 1 本。
#[derive(Debug, Clone, PartialEq)]
pub struct MixInput {
    pub block: AudioBlock,
    /// 役割（`voice`、`bgm`、`sfx`）。バスの振り分けに使う。
    pub role: String,
    /// dB。0 で素通し。
    pub gain_db: f32,
    /// -1（左）..=1（右）。0 で中央。
    pub pan: f32,
}

/// ミキサー。`frames` フレーム分を一度に混ぜる（1 サンプルずつ呼ばない）。
pub trait Mixer: Send {
    /// 出力の形。
    fn sample_rate(&self) -> u32;
    /// 出力のチャンネル数。
    fn channels(&self) -> u16;
    /// 入力を混ぜて `frames` フレーム分を返す。短い入力は無音で埋め、長い入力は切る。
    fn mix(&mut self, inputs: &[MixInput], frames: usize) -> Result<AudioBlock>;
}

/// バス 1 本の設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusConfig {
    /// バスの音量（dB）。
    pub gain_db: f32,
}

/// ダッキング。`trigger` のバスが鳴っている間、`targets` のバスを `reduction_db` だけ下げる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuckingConfig {
    /// 鳴ると他を下げるバスの役割（例: `voice`）。
    pub trigger: String,
    /// 下げられるバスの役割（例: `bgm`）。
    pub targets: Vec<String>,
    /// 下げる量（dB）。負の値。
    pub reduction_db: f32,
    /// 鳴っていると見なす振幅（線形）。
    pub threshold: f32,
    /// 下がり始めてから下がりきるまでの時間（ms）。
    pub attack_ms: f32,
    /// 戻り始めてから戻りきるまでの時間（ms）。
    pub release_ms: f32,
}

impl DuckingConfig {
    /// 声が鳴ったら BGM を 12 dB 下げる、字幕動画向けの既定。
    pub fn voice_over_bgm() -> Self {
        Self {
            trigger: "voice".into(),
            targets: vec!["bgm".into()],
            reduction_db: -12.0,
            threshold: 0.01,
            attack_ms: 20.0,
            release_ms: 300.0,
        }
    }
}

/// ミキサーの設定。既定ではダッキングは無効で、バスは全て 0 dB。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MixerConfig {
    /// 役割ごとのバス。載っていない役割は 0 dB。
    pub buses: BTreeMap<String, BusConfig>,
    /// ダッキング。`None` で無効。
    pub ducking: Option<DuckingConfig>,
    /// マスターの音量（dB）。
    pub master_db: f32,
}

/// ミキサーを作る口。`mm-runtime` が設定で選ぶ。
pub trait MixerFactory: Send + Sync {
    /// 実装の名前（`dsp`、見本は `sample`）。
    fn id(&self) -> &'static str;
    /// 設定を渡してミキサーを作る。
    fn create(
        &self,
        sample_rate: u32,
        channels: u16,
        config: &MixerConfig,
    ) -> Result<Box<dyn Mixer>>;
}

/// 音の出口。プレビュー（cpal）と書き出し（WAV）が実装する。
pub trait AudioSink: Send {
    /// 受け付ける形。
    fn sample_rate(&self) -> u32;
    /// 受け付けるチャンネル数。
    fn channels(&self) -> u16;
    /// ブロックを書く。形が合わなければ `InvalidArgument`。
    fn write(&mut self, block: &AudioBlock) -> Result<()>;
    /// 書き終える。書いたフレーム数を返す。
    fn finish(&mut self) -> Result<u64>;
}

/// ラウドネスのメーター（ITU-R BS.1770）。
pub trait LoudnessMeter: Send {
    /// ブロックを食わせる。
    fn feed(&mut self, block: &AudioBlock) -> Result<()>;
    /// 統合ラウドネス（LUFS）。十分な長さが無ければ `None`。
    fn integrated_lufs(&self) -> Option<f32>;
    /// 計測を最初からにする。
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn フレーム数はチャンネルで割った数() {
        let b = AudioBlock::silence(48_000, 2, 10);
        assert_eq!(b.frames(), 10);
        assert!(b.is_silent());
    }

    #[test]
    fn 割り切れない長さは断る() {
        assert!(AudioBlock::from_samples(48_000, 2, vec![0.0; 3]).is_err());
    }
}
