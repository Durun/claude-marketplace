//! `removeSequence`。sequence を外す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, SequenceId};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{sequence_referrers, AddSequence};
use crate::touches::Touches;

/// sequence を外す。レイヤーの接続、`sequence` の種類、ほかの sequence の `origin` から
/// 参照されていれば `Project::validate` が断る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveSequence {
    pub sequence: SequenceId,
}

impl Apply for RemoveSequence {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let seq = project.sequence(&self.sequence)?;
        let mut t = Touches::of(&seq.id);
        for c in &seq.cuts {
            t.add(&c.id);
        }
        sequence_referrers(project, seq, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        project.sequence(&self.sequence)?;
        let pos = project
            .sequences
            .iter()
            .position(|s| s.id == self.sequence)
            .expect("sequence が引いた");
        let sequence = project.sequences.remove(pos);
        Ok(AddSequence { sequence }.into())
    }
}
