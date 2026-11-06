//! `split`。カットを 2 つに割る。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::{RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::{cut_referrers, invalid, Join};
use crate::touches::Touches;

/// `cut` を、イン点から `at`（シーケンスの時間軸での長さ）の所で割る。後ろの半分が `new_cut` になる。
/// トランジションは前の半分に残り、`origin` は両方にコピーする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Split {
    pub cut: CutId,
    pub at: RationalTime,
    pub new_cut: CutId,
}

impl Apply for Split {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (seq_id, index) = project.cut_index(&self.cut)?;
        let mut t = Touches::of(&seq_id);
        t.add(&self.cut).add(&self.new_cut);
        cut_referrers(project, &self.cut, &mut t);
        // 割った後ろの半分の次になるカット。
        if let Some(next) = project.sequence(&seq_id)?.cuts.get(index + 1) {
            t.add(&next.id);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.cut(&self.new_cut).is_ok() {
            return Err(invalid(format!("カット {} は既に在る", self.new_cut)));
        }
        let (seq_id, index) = project.cut_index(&self.cut)?;
        let seq = project.sequence_mut(&seq_id)?;
        let first = &mut seq.cuts[index];
        let duration = first.duration()?;
        if self.at <= RationalTime::ZERO || self.at >= duration {
            return Err(invalid(format!(
                "割る位置 {} がカット {} の中（0 < at < {duration}）に無い",
                self.at, self.cut
            )));
        }
        let point = first.in_point.checked_add(self.at.scale(first.speed)?)?;
        let mut second = first.clone();
        second.id = self.new_cut.clone();
        second.in_point = point;
        second.transition_in = None;
        first.out_point = point;
        seq.cuts.insert(index + 1, second);
        Ok(Join {
            cut: self.cut.clone(),
            next: self.new_cut.clone(),
        }
        .into())
    }
}
