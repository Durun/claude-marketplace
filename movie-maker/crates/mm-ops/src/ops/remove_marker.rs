//! `removeMarker`。マーカーを外す。

use serde::{Deserialize, Serialize};

use mm_doc::{MarkerId, Project};
use mm_types::{CompositionId, MmError, Result};

use crate::op::{Apply, Op};
use crate::ops::AddMarker;
use crate::touches::Touches;

/// マーカーを外す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveMarker {
    pub marker: MarkerId,
}

/// マーカーの置き場所（コンポジションの ID と位置）。
fn locate(project: &Project, id: &MarkerId) -> Result<(CompositionId, usize)> {
    project
        .compositions
        .iter()
        .find_map(|c| {
            c.markers
                .iter()
                .position(|m| &m.id == id)
                .map(|i| (c.id.clone(), i))
        })
        .ok_or_else(|| MmError::NotFound(format!("marker {id}")))
}

impl Apply for RemoveMarker {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (comp, _) = locate(project, &self.marker)?;
        let mut t = Touches::of(&comp);
        t.add(&self.marker);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (comp_id, index) = locate(project, &self.marker)?;
        let marker = project.composition_mut(&comp_id)?.markers.remove(index);
        Ok(AddMarker {
            composition: comp_id,
            marker,
        }
        .into())
    }
}
