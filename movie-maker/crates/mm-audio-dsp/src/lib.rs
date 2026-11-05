//! 音の実装。ミキサー（音量・パン・バス・ダッキング）と、BS.1770 のラウドネスメーター。
//!
//! 整数ではなく f32 で、CPU だけで回す。プレビューと書き出しで同じコードを使う。

mod meter;
mod mixer;

pub use meter::DspLoudnessMeter;
pub use mixer::{db_to_gain, BusConfig, DspMixer, DspMixerFactory, DuckingConfig, MixerConfig};
