//! `removeSelection`。選別を外す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, SelectionId};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::AddSelection;
use crate::touches::Touches;

/// 選別を外す（`addSelection` の逆操作）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveSelection {
    pub selection: SelectionId,
}

impl Apply for RemoveSelection {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let s = project.selection(&self.selection)?;
        let mut t = Touches::of(&s.id);
        t.add(&s.asset);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        project.selection(&self.selection)?;
        let list = &mut project.bins.selections;
        let pos = list
            .iter()
            .position(|s| s.id == self.selection)
            .expect("selection が引けた");
        Ok(AddSelection {
            selection: list.remove(pos),
        }
        .into())
    }
}
