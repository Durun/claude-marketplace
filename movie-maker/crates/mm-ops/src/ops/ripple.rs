//! `ripple`。カットの頭か尻を動かし、後ろをずらす。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::{RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::negate;
use crate::touches::Touches;

/// 動かす端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    /// 頭（`in`）。
    In,
    /// 尻（`out`）。
    Out,
}

/// `cut` の `edge` を、並びの時間軸で `by` だけ動かす（素材の時間では `by × speed`）。
/// 頭を正に動かすと短くなり、尻を正に動かすと長くなる。後ろのカットは配置でずれる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ripple {
    pub cut: CutId,
    pub edge: Edge,
    pub by: RationalTime,
}

impl Apply for Ripple {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.cut))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let cut = project.cut_mut(&self.cut)?;
        let delta = self.by.scale(cut.speed)?;
        match self.edge {
            Edge::In => cut.in_point = cut.in_point.checked_add(delta)?,
            Edge::Out => cut.out_point = cut.out_point.checked_add(delta)?,
        }
        Ok(Ripple {
            cut: self.cut.clone(),
            edge: self.edge,
            by: negate(self.by)?,
        }
        .into())
    }
}
