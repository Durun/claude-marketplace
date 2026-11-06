//! `slide`。カットを前後のカットの中で動かす。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::{RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::roll::{next_cut, prev_cut};
use crate::ops::{cut_referrers, negate};
use crate::touches::Touches;

/// `cut` を並びの時間軸で `by` だけ動かす。前のカットの尻が伸び（縮み）、次のカットの頭が縮む（伸びる）ので、
/// `cut` の中身と全体の長さは変わらない。前後のカットが要る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Slide {
    pub cut: CutId,
    pub by: RationalTime,
}

impl Apply for Slide {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.cut);
        let (prev, next) = (prev_cut(project, &self.cut)?, next_cut(project, &self.cut)?);
        t.add(&prev).add(&next);
        for id in [&self.cut, &prev, &next] {
            cut_referrers(project, id, &mut t);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let prev_id = prev_cut(project, &self.cut)?;
        let next_id = next_cut(project, &self.cut)?;
        let prev = project.cut_mut(&prev_id)?;
        prev.out_point = prev.out_point.checked_add(self.by.scale(prev.speed)?)?;
        let next = project.cut_mut(&next_id)?;
        next.in_point = next.in_point.checked_add(self.by.scale(next.speed)?)?;
        Ok(Slide {
            cut: self.cut.clone(),
            by: negate(self.by)?,
        }
        .into())
    }
}
