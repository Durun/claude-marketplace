//! `removeLayer`。レイヤーを外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::ops::{invalid, layer_referrers, AddLayer};
use crate::touches::Touches;

/// レイヤーを外す。ほかのレイヤーの親やトラックマットになっていれば断る（先に外す）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveLayer {
    pub layer: LayerId,
}

impl Apply for RemoveLayer {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (comp, _) = project.layer_index(&self.layer)?;
        let mut t = Touches::of(&comp);
        t.add(&self.layer);
        layer_referrers(project.composition(&comp)?, &self.layer, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (comp_id, index) = project.layer_index(&self.layer)?;
        let comp = project.composition_mut(&comp_id)?;
        let referenced = comp.layers.iter().find(|l| {
            l.parent.as_ref() == Some(&self.layer)
                || l.track_matte
                    .as_ref()
                    .is_some_and(|m| m.layer == self.layer)
        });
        if let Some(l) = referenced {
            return Err(invalid(format!(
                "レイヤー {} はレイヤー {} から参照されている",
                self.layer, l.id
            )));
        }
        let layer = comp.layers.remove(index);
        Ok(AddLayer {
            composition: comp_id,
            layer,
            index: Some(index),
        }
        .into())
    }
}
