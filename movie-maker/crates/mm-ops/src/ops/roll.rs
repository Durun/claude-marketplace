//! `roll`。次のカットとの境目を動かす。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::{RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::{cut_referrers, invalid, negate};
use crate::touches::Touches;

/// `cut` とそのすぐ後ろのカットの境目を、シーケンスの時間軸で `by` だけ動かす。
/// `cut` のアウト点と次のカットのイン点が同じ量だけ動くので、全体の長さは変わらない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Roll {
    pub cut: CutId,
    pub by: RationalTime,
}

/// `cut` のすぐ後ろのカットの ID。
pub(crate) fn next_cut(project: &Project, cut: &CutId) -> Result<CutId> {
    let (seq_id, index) = project.cut_index(cut)?;
    project
        .sequence(&seq_id)?
        .cuts
        .get(index + 1)
        .map(|c| c.id.clone())
        .ok_or_else(|| invalid(format!("カット {cut} の後ろにカットが無い")))
}

/// `cut` のすぐ前のカットの ID。
pub(crate) fn prev_cut(project: &Project, cut: &CutId) -> Result<CutId> {
    let (seq_id, index) = project.cut_index(cut)?;
    index
        .checked_sub(1)
        .and_then(|i| {
            project
                .sequence(&seq_id)
                .ok()?
                .cuts
                .get(i)
                .map(|c| c.id.clone())
        })
        .ok_or_else(|| invalid(format!("カット {cut} の前にカットが無い")))
}

impl Apply for Roll {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.cut);
        t.add(&next_cut(project, &self.cut)?);
        cut_referrers(project, &self.cut, &mut t);
        cut_referrers(project, &next_cut(project, &self.cut)?, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let next_id = next_cut(project, &self.cut)?;
        let cut = project.cut_mut(&self.cut)?;
        cut.out_point = cut.out_point.checked_add(self.by.scale(cut.speed)?)?;
        let next = project.cut_mut(&next_id)?;
        next.in_point = next.in_point.checked_add(self.by.scale(next.speed)?)?;
        Ok(Roll {
            cut: self.cut.clone(),
            by: negate(self.by)?,
        }
        .into())
    }
}
