//! `setCaption`。sequence のレイヤーの、そのコンポジションでの字幕の置き場所を置く・外す。

use serde::{Deserialize, Serialize};

use mm_doc::{CaptionPlacement, Project};
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::touches::Touches;

/// レイヤー `layer` の字幕の置き場所を `caption` にする（`None` で字幕を描く側の既定に戻す）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCaption {
    pub layer: LayerId,
    #[serde(default)]
    pub caption: Option<CaptionPlacement>,
}

impl Apply for SetCaption {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.layer))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let layer = project.layer_mut(&self.layer)?;
        let old = std::mem::replace(&mut layer.framing.caption, self.caption.clone());
        Ok(SetCaption {
            layer: self.layer.clone(),
            caption: old,
        }
        .into())
    }
}
