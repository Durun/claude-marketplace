//! `setMixer`。コンポジションのミキサーの設定（バスの音量、マスター、ダッキング）をまとめて置く・外す。

use serde::{Deserialize, Serialize};

use mm_doc::{CompMixer, Project};
use mm_types::{CompositionId, Result};

use crate::op::{Apply, Op};
use crate::touches::Touches;

/// コンポジション `composition` のミキサーの設定を `mixer` にする（`None` で既定の全部 0 dB、ダッキングなしに戻す）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMixer {
    pub composition: CompositionId,
    #[serde(default)]
    pub mixer: Option<CompMixer>,
}

impl Apply for SetMixer {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.composition))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let comp = project.composition_mut(&self.composition)?;
        let old = std::mem::replace(&mut comp.mixer, self.mixer.clone());
        Ok(SetMixer {
            composition: self.composition.clone(),
            mixer: old,
        }
        .into())
    }
}
