//! `removeMask`。レイヤーからマスクを外す。

use serde::{Deserialize, Serialize};

use mm_doc::{MaskId, Project};
use mm_types::{LayerId, MmError, Result};

use crate::op::{Apply, Op};
use crate::ops::AddMask;
use crate::touches::Touches;

/// マスクを外す。マスクの ID はプロジェクト全体で重ならないので、持ち主のレイヤーは探して決める。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveMask {
    pub mask: MaskId,
}

/// マスクの持ち主のレイヤーと、列の中の位置。
fn locate(project: &Project, id: &MaskId) -> Result<(LayerId, usize)> {
    project
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .find_map(|l| {
            l.masks
                .iter()
                .position(|m| &m.id == id)
                .map(|i| (l.id.clone(), i))
        })
        .ok_or_else(|| MmError::NotFound(format!("mask {id}")))
}

impl Apply for RemoveMask {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (layer, _) = locate(project, &self.mask)?;
        let mut t = Touches::of(&layer);
        t.add(&self.mask);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (layer_id, index) = locate(project, &self.mask)?;
        let mask = project.layer_mut(&layer_id)?.masks.remove(index);
        Ok(AddMask {
            layer: layer_id,
            mask,
            index: Some(index),
        }
        .into())
    }
}
