//! `overwrite`。カットを別のカットで置き換える。

use serde::{Deserialize, Serialize};

use mm_doc::{Cut, CutId, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_referrers, cut_refs_reach, cuts_beside, invalid};
use crate::touches::Touches;

/// `cut` を `replacement` で置き換える。シーケンスのカットの数と位置は変わらない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Overwrite {
    pub cut: CutId,
    pub replacement: Cut,
}

impl Apply for Overwrite {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (seq_id, index) = project.cut_index(&self.cut)?;
        let mut t = Touches::of(&self.cut);
        t.add(&seq_id);
        t.add(&self.replacement.id);
        cut_refs_reach(project, &self.replacement, &mut t);
        cuts_beside(project.sequence(&seq_id)?, index, &mut t);
        cut_referrers(project, &self.cut, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if self.replacement.id != self.cut && project.cut(&self.replacement.id).is_ok() {
            return Err(invalid(format!(
                "カット {} は既に在る",
                self.replacement.id
            )));
        }
        let cut = project.cut_mut(&self.cut)?;
        let old = std::mem::replace(cut, self.replacement.clone());
        Ok(Overwrite {
            cut: self.replacement.id.clone(),
            replacement: old,
        }
        .into())
    }
}
