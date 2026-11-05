//! `insert`。カットを並びに差し込む。

use serde::{Deserialize, Serialize};

use mm_doc::{Cut, Project, SequenceId};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_refs, cuts_around_gap, invalid, Extract};
use crate::touches::Touches;

/// `sequence` の `index`（頭から数えた位置）に `cut` を差し込む。後ろのカットはずれる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Insert {
    pub sequence: SequenceId,
    pub index: usize,
    pub cut: Cut,
}

impl Apply for Insert {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.sequence);
        t.add(&self.cut.id);
        cut_refs(&self.cut, &mut t);
        cuts_around_gap(project.sequence(&self.sequence)?, self.index, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.cut(&self.cut.id).is_ok() {
            return Err(invalid(format!("カット {} は既に在る", self.cut.id)));
        }
        let seq = project.sequence_mut(&self.sequence)?;
        if self.index > seq.cuts.len() {
            return Err(invalid(format!(
                "カットの位置 {} が {} を超える",
                self.index,
                seq.cuts.len()
            )));
        }
        seq.cuts.insert(self.index, self.cut.clone());
        Ok(Extract {
            cut: self.cut.id.clone(),
        }
        .into())
    }
}
