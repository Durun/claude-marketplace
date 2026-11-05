//! `moveLayer`。レイヤーの重なりの位置を変える。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::ops::invalid;
use crate::touches::Touches;

/// レイヤーを同じコンポジションの中の `index`（下から数えた位置）に動かす。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveLayer {
    pub layer: LayerId,
    pub index: usize,
}

impl Apply for MoveLayer {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (comp, _) = project.layer_index(&self.layer)?;
        let mut t = Touches::of(&comp);
        t.add(&self.layer);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (comp_id, from) = project.layer_index(&self.layer)?;
        let comp = project.composition_mut(&comp_id)?;
        if self.index >= comp.layers.len() {
            return Err(invalid(format!(
                "レイヤーの位置 {} が {} 以上",
                self.index,
                comp.layers.len()
            )));
        }
        let layer = comp.layers.remove(from);
        comp.layers.insert(self.index, layer);
        Ok(MoveLayer {
            layer: self.layer.clone(),
            index: from,
        }
        .into())
    }
}
