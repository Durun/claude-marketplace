//! ミキサーの契約テスト。

use std::sync::Arc;

use mm_audio_dsp::DspMixerFactory;
use mm_testkit::audio::contract;

#[test]
fn 契約を満たす() {
    contract(|| Arc::new(DspMixerFactory));
}
