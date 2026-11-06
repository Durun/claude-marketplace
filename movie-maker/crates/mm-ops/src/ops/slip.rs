//! `slip`。カットの中身をずらす。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::{RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::{cut_referrers, negate};
use crate::touches::Touches;

/// `cut` の `in` と `out` を、並びの時間軸で `by` だけ同じ量ずらす（素材の時間では `by × speed`）。
/// 並びの上での位置と長さは変わらない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Slip {
    pub cut: CutId,
    pub by: RationalTime,
}

impl Apply for Slip {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.cut);
        cut_referrers(project, &self.cut, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let cut = project.cut_mut(&self.cut)?;
        let delta = self.by.scale(cut.speed)?;
        cut.in_point = cut.in_point.checked_add(delta)?;
        cut.out_point = cut.out_point.checked_add(delta)?;
        Ok(Slip {
            cut: self.cut.clone(),
            by: negate(self.by)?,
        }
        .into())
    }
}
