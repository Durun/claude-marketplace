//! `addSelection`。選別を足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, Selection};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveSelection};
use crate::touches::Touches;

/// 選別（素材の中の範囲と評価とタグ）を足す。同じ ID の選別か話題が在れば拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddSelection {
    pub selection: Selection,
}

impl Apply for AddSelection {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.selection.id);
        t.add(&self.selection.asset);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let id = &self.selection.id;
        if project.selection(id).is_ok() {
            return Err(invalid(format!("選別 {id} は既に在る")));
        }
        let list = &mut project.bins.selections;
        let pos = list.partition_point(|s| s.id < *id);
        list.insert(pos, self.selection.clone());
        Ok(RemoveSelection {
            selection: id.clone(),
        }
        .into())
    }
}
