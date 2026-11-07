//! `addMask`。レイヤーにマスクを足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Mask, Project};
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveMask};
use crate::touches::Touches;

/// レイヤー `layer` のマスクの列の `index` の位置（省くと末尾）に `mask` を差し込む。
/// マスクは上から順に組み合わせるので、位置で結果が変わる。`removeMask` の逆操作が元の位置に戻す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddMask {
    pub layer: LayerId,
    pub mask: Mask,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

impl Apply for AddMask {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.layer);
        t.add(&self.mask.id);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let layer = project.layer_mut(&self.layer)?;
        let index = self.index.unwrap_or(layer.masks.len());
        if index > layer.masks.len() {
            return Err(invalid(format!(
                "マスクの位置 {index} がレイヤー {} のマスクの数 {} を超える",
                self.layer,
                layer.masks.len()
            )));
        }
        layer.masks.insert(index, self.mask.clone());
        Ok(RemoveMask {
            mask: self.mask.id.clone(),
        }
        .into())
    }
}
