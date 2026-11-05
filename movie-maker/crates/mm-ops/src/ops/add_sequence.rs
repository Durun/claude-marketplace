//! `addSequence`。sequence を足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, Sequence};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_refs, invalid, RemoveSequence};
use crate::touches::Touches;

/// sequence を足す（カットを含めてよい）。本編の並びはこれで作る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddSequence {
    pub sequence: Sequence,
}

impl Apply for AddSequence {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.sequence.id);
        for c in &self.sequence.cuts {
            t.add(&c.id);
            cut_refs(c, &mut t);
        }
        if let Some(o) = &self.sequence.origin {
            t.add(&o.sequence);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.sequence(&self.sequence.id).is_ok() {
            return Err(invalid(format!("sequence {} は既に在る", self.sequence.id)));
        }
        project.sequences.push(self.sequence.clone());
        Ok(RemoveSequence {
            sequence: self.sequence.id.clone(),
        }
        .into())
    }
}
