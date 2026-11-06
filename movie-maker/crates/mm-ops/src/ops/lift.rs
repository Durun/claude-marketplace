//! `lift`。カットを抜き、抜いた所に同じ長さの隙間を残す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Node, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_refs_reach, invalid, Overwrite};
use crate::touches::Touches;

/// `cut` のノードを隙間（[`Node::gap`]。透明で音の無い単色）に替える。ID、イン点とアウト点、速さ、
/// トランジションは残すので、シーケンスの長さも後ろのカットの位置も変わらない（OTIO の `Gap`、FCP の隙間のクリップと同じ扱い）。
/// 逆操作は元のカットへの `overwrite`。隙間のカットは拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lift {
    pub cut: CutId,
}

impl Apply for Lift {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (_, cut) = project.cut(&self.cut)?;
        let mut t = Touches::of(&self.cut);
        // 元のノードの入れ子の参照が消えるので、その先を読む操作と重ねる。
        cut_refs_reach(project, cut, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let cut = project.cut_mut(&self.cut)?;
        if cut.source.is_gap() {
            return Err(invalid(format!("カット {} は既に隙間", self.cut)));
        }
        let old = cut.clone();
        cut.source = Node::gap();
        Ok(Overwrite {
            cut: self.cut.clone(),
            replacement: old,
        }
        .into())
    }
}
