//! `insertCuts`。続いたカットの列をシーケンスに差し込む。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use mm_doc::{Cut, Project, SequenceId};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_refs_reach, cuts_around_gap, invalid, ExtractCuts};
use crate::touches::Touches;

/// `sequence` の `index` に `cuts` を順に差し込む。後ろのカットはずれる。貼り付けと `extractCuts` の逆操作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertCuts {
    pub sequence: SequenceId,
    pub index: usize,
    pub cuts: Vec<Cut>,
}

impl Apply for InsertCuts {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.sequence);
        for cut in &self.cuts {
            t.add(&cut.id);
            cut_refs_reach(project, cut, &mut t);
        }
        cuts_around_gap(project.sequence(&self.sequence)?, self.index, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        splice(project, &self.sequence, self.index, &self.cuts)?;
        Ok(ExtractCuts {
            sequence: self.sequence.clone(),
            index: self.index,
            count: self.cuts.len(),
        }
        .into())
    }
}

/// `cuts` を `index` に差し込む。空の列、在る ID、列の中の重複、末尾を超える位置は拒否する。
pub(crate) fn splice(
    project: &mut Project,
    sequence: &SequenceId,
    index: usize,
    cuts: &[Cut],
) -> Result<()> {
    if cuts.is_empty() {
        return Err(invalid("差し込むカットが無い"));
    }
    let mut seen = HashSet::new();
    for cut in cuts {
        if !seen.insert(cut.id.as_str()) || project.cut(&cut.id).is_ok() {
            return Err(invalid(format!("カット {} は既に在る", cut.id)));
        }
    }
    let seq = project.sequence_mut(sequence)?;
    if index > seq.cuts.len() {
        return Err(invalid(format!(
            "カットの位置 {index} が {} を超える",
            seq.cuts.len()
        )));
    }
    seq.cuts.splice(index..index, cuts.iter().cloned());
    Ok(())
}
