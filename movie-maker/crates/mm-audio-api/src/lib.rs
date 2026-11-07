//! 音のインターフェース。ミキサーは Float32、サンプル単位、インターリーブで、プレビューと書き出しで同じ DSP を回す。
//!
//! 契約（`mm_testkit::audio::contract`）:
//! - 決定性: 同じ入力を 2 回混ぜると、サンプル列が bit で一致する。
//! - 無音の入力は無音を返す。
//! - 入力 1 本、0 dB、パン中央はパススルー（許容差 1e-6）。
//! - 返すブロックの `frames()` は頼んだ `frames` と一致する。
//! - バスの音量（`buses[role].gain_db`）とマスター（`master_db`）は全実装が効かせる。ダッキングは実装が省いてよい（モックは省く）。

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
    /// dB。0 でパススルー。
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
    /// 直前の `mix` の入力ごとのピーク（線形の振幅）。音量とパンをかけた後、バスとダッキングの前で、`inputs` と同じ順。
    /// Mixer のトラックのメーターが使う。測らない実装は空（既定）。
    fn input_peaks(&self) -> &[f32] {
        &[]
    }
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
    /// 対象のバス（下げられる側）の役割（例: `bgm`）。
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

/// ミキサーを作るファクトリー。`mm-runtime` が設定で選ぶ。
pub trait MixerFactory: Send + Sync {
    /// 実装の名前（`dsp`、モックは `sample`）。
    fn id(&self) -> &'static str;
    /// 設定を渡してミキサーを作る。
    fn create(
        &self,
        sample_rate: u32,
        channels: u16,
        config: &MixerConfig,
    ) -> Result<Box<dyn Mixer>>;

    /// 書き出しのラウドネスの正規化。対応しない実装は `None`（既定）で、呼ぶ側が拒否する。
    fn loudness_normalizer(&self) -> Option<Box<dyn LoudnessNormalizer>> {
        None
    }

    /// プレビューの Mixer のマスターのメーター。対応しない実装は `None`（既定）で、メーターを出さない。
    fn master_meter(&self, sample_rate: u32, channels: u16) -> Option<Box<dyn MasterMeter>> {
        let _ = (sample_rate, channels);
        None
    }
}

/// マスターのメーターの 1 ブロック分の読み。ピークは線形の振幅、ラウドネスは LUFS。測れなければ `None`。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MasterReading {
    /// このブロックのチャンネルごとのサンプルのピーク。
    pub peak: Vec<f32>,
    /// `reset` からのトゥルーピークの最大（全チャンネル）。
    pub true_peak: Option<f32>,
    /// 短期ラウドネス（直近 3 秒）。
    pub short_term_lufs: Option<f32>,
    /// 統合ラウドネス（`reset` から）。
    pub integrated_lufs: Option<f32>,
}

/// Mixer のマスターのメーター（ITU-R BS.1770）。再生中にミキサーの出力を順に食わせる。
pub trait MasterMeter: Send {
    /// ブロックを食わせ、その時点の読みを返す。形が違えば `InvalidArgument`。
    fn feed(&mut self, block: &AudioBlock) -> Result<MasterReading>;
    /// 計測を最初からにする（シークしたとき）。
    fn reset(&mut self);
}

/// ラウドネスの正規化の指定。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizeSpec {
    /// 統合ラウドネスの目標（LUFS）。
    pub target_lufs: f32,
    /// トゥルーピークの上限（dBTP）。
    pub ceiling_dbtp: f32,
}

/// 正規化の結果。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizeReport {
    /// かける前の統合ラウドネス（LUFS）。無音などで測れなければ `None`（このとき音は変えない）。
    pub measured_lufs: Option<f32>,
    /// かけた後の統合ラウドネス（LUFS）。
    pub final_lufs: Option<f32>,
    /// 最初にかけた倍率（dB）。リミッターの前の値。
    pub gain_db: f32,
    /// かけた後のトゥルーピーク（dBTP）。
    pub true_peak_dbtp: f32,
    /// リミッターが音を削ったか。
    pub limited: bool,
}

/// 書き出しのラウドネスの正規化。1 パス目で測り、2 パス目で倍率とリミッターをかける。
pub trait LoudnessNormalizer: Send {
    /// `block` を、統合ラウドネスが `spec.target_lufs`、トゥルーピークが `spec.ceiling_dbtp` 以下になるよう直す。
    fn normalize(&self, block: &mut AudioBlock, spec: &NormalizeSpec) -> Result<NormalizeReport>;
}

/// 音の出力先。プレビュー（cpal）と書き出し（WAV）が実装する。
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
    fn 割り切れない長さは拒否する() {
        assert!(AudioBlock::from_samples(48_000, 2, vec![0.0; 3]).is_err());
    }
}
