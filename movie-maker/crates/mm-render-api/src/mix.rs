//! ミキサーの設定とメーター。レンダラーの実装（wgpu、モック）が同じ変換を使い、プレビューと書き出しで同じ音にする。

use mm_audio_api::{BusConfig, DuckingConfig, MixerConfig};
use mm_doc::{CompMixer, Ducking};
use mm_types::LayerId;

/// トラック 1 本のメーターの値。直前の `render_audio` の、開いたコンポジションの直下のレイヤーのもの。
#[derive(Debug, Clone, PartialEq)]
pub struct TrackLevel {
    pub layer: LayerId,
    /// 音量とパンの後のピーク（線形の振幅）。
    pub peak: f32,
}

/// dBFS を線形の振幅に。
fn db_to_amplitude(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// プロジェクトのデータのミキサーの設定を、ミキサーの設定に変える。`ducking` が偽ならダッキングを外す
/// （入れ子は区間ごとに作り直すミキサーで混ぜるので、検出の状態を持ち越せない）。
pub fn mixer_config(mixer: Option<&CompMixer>, ducking: bool) -> MixerConfig {
    let Some(m) = mixer else {
        return MixerConfig::default();
    };
    MixerConfig {
        buses: m
            .buses
            .iter()
            .map(|(role, bus)| {
                (
                    role.clone(),
                    BusConfig {
                        gain_db: bus.gain_db as f32,
                    },
                )
            })
            .collect(),
        ducking: m.ducking.as_ref().filter(|_| ducking).map(ducking_config),
        master_db: m.master_db as f32,
    }
}

fn ducking_config(d: &Ducking) -> DuckingConfig {
    DuckingConfig {
        trigger: d.trigger.clone(),
        targets: d.targets.clone(),
        reduction_db: d.reduction_db as f32,
        threshold: db_to_amplitude(d.threshold_db),
        attack_ms: d.attack_ms as f32,
        release_ms: d.release_ms as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 設定が無ければ既定() {
        assert_eq!(mixer_config(None, true), MixerConfig::default());
    }

    #[test]
    fn しきい値の_db_は振幅にし_入れ子ではダッキングを外す() {
        let m = CompMixer {
            master_db: -2.0,
            buses: [("bgm".to_owned(), mm_doc::MixerBus { gain_db: -6.0 })].into(),
            ducking: Some(Ducking::voice_over_bgm()),
        };
        let c = mixer_config(Some(&m), true);
        assert_eq!(c.master_db, -2.0);
        assert_eq!(c.buses["bgm"].gain_db, -6.0);
        let d = c.ducking.expect("ダッキング");
        assert!((d.threshold - 0.01).abs() < 1e-6, "-40 dBFS は 0.01");
        assert_eq!(d, {
            let mut want = DuckingConfig::voice_over_bgm();
            want.threshold = d.threshold;
            want
        });
        assert!(mixer_config(Some(&m), false).ducking.is_none());
    }
}
