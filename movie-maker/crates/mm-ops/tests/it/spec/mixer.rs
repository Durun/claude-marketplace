//! Mixer の操作（レイヤーの音の設定、コンポジションのミキサーの設定）の種。

use super::build::{self, layer_id, pick};
use mm_doc::Project;
use mm_ops::*;
use proptest::prelude::*;

/// Mixer の操作の種。
#[derive(Debug, Clone)]
pub enum MixSpec {
    /// レイヤーの番号と、音の設定の種（0 は外す、奇数はミュート、4 以上はソロ）。
    LayerAudio(u8, u8),
    /// コンポジションの番号と、ミキサーの設定の種（0 は外す、偶数はダッキングあり）。
    Mixer(u8, u8),
}

pub fn arb() -> impl Strategy<Value = MixSpec> {
    let s = || any::<u8>();
    prop_oneof![
        (s(), s()).prop_map(|(a, b)| MixSpec::LayerAudio(a, b)),
        (s(), s()).prop_map(|(a, b)| MixSpec::Mixer(a, b)),
    ]
}

pub fn op(spec: &MixSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        MixSpec::LayerAudio(i, seed) => SetLayerAudio {
            layer: layer_id(p, *i)?,
            audio: build::layer_audio(*seed),
        }
        .into(),
        MixSpec::Mixer(i, seed) => SetMixer {
            composition: pick(&p.compositions, *i)?.id.clone(),
            mixer: build::comp_mixer(*seed),
        }
        .into(),
    })
}
