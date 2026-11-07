//! コンポジションのミキサーの設定。役割ごとのバスの音量、マスターの音量、ダッキング。
//!
//! レイヤーごとの音量・パン・ミュート・ソロ・送り先のバス（役割）は [`crate::LayerAudio`] が持つ。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// ダッキングの既定の下げる量（dB）。字幕動画で声の後ろの BGM を聞こえる程度に残す量。
pub const DEFAULT_DUCK_REDUCTION_DB: f64 = -12.0;
/// ダッキングの既定の鳴っていると見なす大きさ（dBFS）。-40 dBFS は振幅 0.01。
pub const DEFAULT_DUCK_THRESHOLD_DB: f64 = -40.0;
/// ダッキングの既定の下がりきるまでの時間（ms）。
pub const DEFAULT_DUCK_ATTACK_MS: f64 = 20.0;
/// ダッキングの既定の戻りきるまでの時間（ms）。
pub const DEFAULT_DUCK_RELEASE_MS: f64 = 300.0;

/// バスとマスターの音量の上限（dB）。ミキサーは `f32` の振幅に直すので、大きすぎる値は無限大になって音が壊れる。
/// 卓のフェーダーの上限（+6〜+12 dB）より余裕を持たせる。
pub(crate) const MAX_MIXER_GAIN_DB: f64 = 24.0;

/// コンポジションのミキサーの設定。既定は全部 0 dB でダッキングなし。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompMixer {
    /// マスターの音量（dB）。
    #[serde(default)]
    pub master_db: f64,
    /// 役割ごとのバス。載っていない役割は 0 dB。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub buses: BTreeMap<String, MixerBus>,
    /// ダッキング。`None` で無効。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ducking: Option<Ducking>,
}

/// バス 1 本の設定。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MixerBus {
    /// バスの音量（dB）。
    pub gain_db: f64,
}

/// ダッキング。`trigger` のバスが鳴っている間、`targets` のバスを `reduction_db` だけ下げる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Ducking {
    /// 鳴ると他を下げるバスの役割（例: `voice`）。
    pub trigger: String,
    /// 下げられるバスの役割（例: `bgm`）。
    pub targets: Vec<String>,
    /// 下げる量（dB）。0 以下。
    pub reduction_db: f64,
    /// 鳴っていると見なす大きさ（dBFS）。
    pub threshold_db: f64,
    /// 下がりきるまでの時間（ms）。
    pub attack_ms: f64,
    /// 戻りきるまでの時間（ms）。
    pub release_ms: f64,
}

impl Ducking {
    /// 声が鳴ったら BGM を下げる、字幕動画向けの既定。
    pub fn voice_over_bgm() -> Self {
        Self {
            trigger: "voice".into(),
            targets: vec!["bgm".into()],
            reduction_db: DEFAULT_DUCK_REDUCTION_DB,
            threshold_db: DEFAULT_DUCK_THRESHOLD_DB,
            attack_ms: DEFAULT_DUCK_ATTACK_MS,
            release_ms: DEFAULT_DUCK_RELEASE_MS,
        }
    }
}

impl CompMixer {
    /// 不変条件。音量は有限で [`MAX_MIXER_GAIN_DB`] 以下、ダッキングは下げる向きで時間は負でなく、自分のバスを対象にしない。
    pub(crate) fn check(&self) -> std::result::Result<(), String> {
        let ok = |db: f64| db.is_finite() && db <= MAX_MIXER_GAIN_DB;
        if !ok(self.master_db) {
            return Err(format!(
                "マスターの音量は有限で {MAX_MIXER_GAIN_DB} dB 以下"
            ));
        }
        if let Some((role, _)) = self.buses.iter().find(|(_, b)| !ok(b.gain_db)) {
            return Err(format!(
                "バス {role} の音量は有限で {MAX_MIXER_GAIN_DB} dB 以下"
            ));
        }
        self.ducking.as_ref().map_or(Ok(()), Ducking::check)
    }
}

impl Ducking {
    fn check(&self) -> std::result::Result<(), String> {
        let finite = [
            self.reduction_db,
            self.threshold_db,
            self.attack_ms,
            self.release_ms,
        ]
        .iter()
        .all(|v| v.is_finite());
        if !finite {
            return Err("ダッキングの数が有限でない".into());
        }
        if self.reduction_db > 0.0 {
            return Err("ダッキングの下げる量は 0 dB 以下".into());
        }
        if self.attack_ms < 0.0 || self.release_ms < 0.0 {
            return Err("ダッキングの時間は 0 以上".into());
        }
        if self.targets.contains(&self.trigger) {
            return Err(format!(
                "ダッキングの対象にきっかけのバス {} がある",
                self.trigger
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 既定のミキサーは空の_json_になる() {
        let json = serde_json::to_string(&CompMixer::default()).unwrap();
        assert_eq!(json, r#"{"master_db":0.0}"#);
    }

    #[test]
    fn 上限を超える音量は拒否する() {
        for db in [1e39, MAX_MIXER_GAIN_DB + 1.0] {
            let master = CompMixer {
                master_db: db,
                ..CompMixer::default()
            };
            assert!(master.check().is_err(), "{db}");
            let bus = CompMixer {
                buses: [("bgm".to_owned(), MixerBus { gain_db: db })].into(),
                ..CompMixer::default()
            };
            assert!(bus.check().is_err(), "{db}");
        }
        let edge = CompMixer {
            master_db: MAX_MIXER_GAIN_DB,
            ..CompMixer::default()
        };
        assert!(edge.check().is_ok());
    }

    #[test]
    fn 上げる向きのダッキングは拒否する() {
        let mixer = CompMixer {
            ducking: Some(Ducking {
                reduction_db: 3.0,
                ..Ducking::voice_over_bgm()
            }),
            ..CompMixer::default()
        };
        assert!(mixer.check().is_err());
    }

    #[test]
    fn きっかけのバスを対象にするダッキングは拒否する() {
        let mut duck = Ducking::voice_over_bgm();
        duck.targets.push("voice".into());
        let mixer = CompMixer {
            ducking: Some(duck),
            ..CompMixer::default()
        };
        assert!(mixer.check().is_err());
    }
}
