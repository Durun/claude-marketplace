//! `addMarker`。マーカーを足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Marker, Project};
use mm_types::{CompositionId, Result};

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveMarker};
use crate::touches::Touches;

/// マーカーを時刻の順の位置に差し込む。同じ時刻のマーカーが既にあれば断る（順が決まらない）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddMarker {
    pub composition: CompositionId,
    pub marker: Marker,
}

impl Apply for AddMarker {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.composition);
        t.add(&self.marker.id);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let comp = project.composition_mut(&self.composition)?;
        if comp.markers.iter().any(|m| m.time == self.marker.time) {
            return Err(invalid(format!(
                "時刻 {} にマーカーが既に在る",
                self.marker.time
            )));
        }
        let pos = comp.markers.partition_point(|m| m.time < self.marker.time);
        comp.markers.insert(pos, self.marker.clone());
        Ok(RemoveMarker {
            marker: self.marker.id.clone(),
        }
        .into())
    }
}
