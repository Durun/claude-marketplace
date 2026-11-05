//! ミキサーの契約テスト。

use std::sync::Arc;

use mm_audio_dsp::{DspMixerFactory, DuckingConfig, MixerConfig};
use mm_testkit::audio::contract;

#[test]
fn 既定の設定で契約を満たす() {
    contract(|| Arc::new(DspMixerFactory::default()));
}

#[test]
fn ダッキングを有効にしても契約を満たす() {
    let config = MixerConfig {
        ducking: Some(DuckingConfig::voice_over_bgm()),
        ..MixerConfig::default()
    };
    contract(|| Arc::new(DspMixerFactory::new(config.clone())));
}
