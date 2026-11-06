//! `addLayer`。レイヤーをコンポジションに足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Layer, Project};
use mm_types::{CompositionId, Result};

use crate::op::{Apply, Op};
use crate::ops::{invalid, layer_refs_reach, RemoveLayer};
use crate::touches::Touches;

/// レイヤーを足す。`index` は下から数えた位置で、無ければ一番上。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddLayer {
    pub composition: CompositionId,
    pub layer: Layer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

impl Apply for AddLayer {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.composition);
        t.add(&self.layer.id);
        layer_refs_reach(project, &self.layer, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.layer(&self.layer.id).is_ok() {
            return Err(invalid(format!("レイヤー {} は既に在る", self.layer.id)));
        }
        let comp = project.composition_mut(&self.composition)?;
        let index = self.index.unwrap_or(comp.layers.len());
        if index > comp.layers.len() {
            return Err(invalid(format!(
                "レイヤーの位置 {index} が {} を超える",
                comp.layers.len()
            )));
        }
        comp.layers.insert(index, self.layer.clone());
        Ok(RemoveLayer {
            layer: self.layer.id.clone(),
        }
        .into())
    }
}
