//! `join`。続きの 2 つのカットを 1 つに戻す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, Split};
use crate::touches::Touches;

/// `cut` とそのすぐ後ろの `next` を 1 つにする。同じ素材・速さ・有効で、`cut.out == next.in`、
/// `next` にトランジションが無く、`origin` が同じときだけ戻せる（`split` の逆）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Join {
    pub cut: CutId,
    pub next: CutId,
}

impl Apply for Join {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (seq_id, index) = project.cut_index(&self.cut)?;
        let mut t = Touches::of(&seq_id);
        t.add(&self.cut).add(&self.next);
        // 戻した後の次になるカット。
        if let Some(after) = project.sequence(&seq_id)?.cuts.get(index + 2) {
            t.add(&after.id);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (seq_id, index) = project.cut_index(&self.cut)?;
        let seq = project.sequence_mut(&seq_id)?;
        let Some(next) = seq.cuts.get(index + 1).filter(|c| c.id == self.next) else {
            return Err(invalid(format!(
                "カット {} はカット {} のすぐ後ろでない",
                self.next, self.cut
            )));
        };
        let first = &seq.cuts[index];
        let continuous = first.source == next.source
            && first.speed == next.speed
            && first.enabled == next.enabled
            && first.out_point == next.in_point
            && next.transition_in.is_none()
            && first.origin == next.origin;
        if !continuous {
            return Err(invalid(format!(
                "カット {} と {} は続きでない",
                self.cut, self.next
            )));
        }
        let at = first.duration()?;
        let out = next.out_point;
        seq.cuts.remove(index + 1);
        seq.cuts[index].out_point = out;
        Ok(Split {
            cut: self.cut.clone(),
            at,
            new_cut: self.next.clone(),
        }
        .into())
    }
}
