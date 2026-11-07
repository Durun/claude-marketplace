//! `setLayerAudio`。レイヤーの音の設定（送り先のバス、音量、パン、ミュート、ソロ）をまとめて置く・外す。

use serde::{Deserialize, Serialize};

use mm_doc::{LayerAudio, Project};
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::touches::Touches;

/// レイヤー `layer` の音の設定を `audio` にする（`None` で設定を外し、既定の役割・0 dB・中央に戻す）。
/// Mixer のフェーダーやボタンは変えた後の設定の全体を送り、取り消し 1 回で前の設定に戻る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLayerAudio {
    pub layer: LayerId,
    #[serde(default)]
    pub audio: Option<LayerAudio>,
}

impl Apply for SetLayerAudio {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.layer))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let layer = project.layer_mut(&self.layer)?;
        let old = std::mem::replace(&mut layer.audio, self.audio.clone());
        Ok(SetLayerAudio {
            layer: self.layer.clone(),
            audio: old,
        }
        .into())
    }
}
