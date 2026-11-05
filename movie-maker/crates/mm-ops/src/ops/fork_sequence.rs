//! `forkSequence`。sequence を参照か複製で分岐する。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, OriginRelation, Project, Sequence, SequenceId, SequenceOrigin};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_refs, invalid, RemoveSequence};
use crate::touches::Touches;

/// `sequence` を写して `new_sequence` を作る。カットは `cuts` の ID で写し（数が合うこと）、
/// 各カットの `origin` に元のカットを残す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkSequence {
    pub sequence: SequenceId,
    pub new_sequence: SequenceId,
    pub relation: OriginRelation,
    /// 新しいカットの ID。元のカットと同じ数。
    pub cuts: Vec<CutId>,
}

impl Apply for ForkSequence {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let source = project.sequence(&self.sequence)?;
        let mut t = Touches::of(&self.sequence);
        t.add(&self.new_sequence);
        for c in &source.cuts {
            t.add(&c.id);
            cut_refs(c, &mut t);
        }
        for c in &self.cuts {
            t.add(c);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.sequence(&self.new_sequence).is_ok() {
            return Err(invalid(format!(
                "sequence {} は既に在る",
                self.new_sequence
            )));
        }
        let source = project.sequence(&self.sequence)?;
        if source.cuts.len() != self.cuts.len() {
            return Err(invalid(format!(
                "カットの ID の数 {} が元の {} と合わない",
                self.cuts.len(),
                source.cuts.len()
            )));
        }
        let cuts = source
            .cuts
            .iter()
            .zip(&self.cuts)
            .map(|(cut, id)| {
                let mut c = cut.clone();
                c.id = id.clone();
                c.origin = Some(cut.id.clone());
                c
            })
            .collect();
        project.sequences.push(Sequence {
            id: self.new_sequence.clone(),
            cuts,
            origin: Some(SequenceOrigin {
                sequence: self.sequence.clone(),
                relation: self.relation,
            }),
        });
        Ok(RemoveSequence {
            sequence: self.new_sequence.clone(),
        }
        .into())
    }
}
