//! 音の実装。ミキサー（音量・パン・バス・ダッキング）と、BS.1770 のラウドネスメーター。
//!
//! 整数ではなく f32 で、CPU だけで回す。プレビューと書き出しで同じコードを使う。

mod meter;
mod mixer;
mod normalize;

pub use meter::{DspLoudnessMeter, DspMasterMeter};
pub use mixer::{db_to_gain, DspMixer, DspMixerFactory};
pub use normalize::{limit, measure, DspLoudnessNormalizer, Measurement};
