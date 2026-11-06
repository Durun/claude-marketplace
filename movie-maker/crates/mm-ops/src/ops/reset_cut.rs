//! `resetCut`。参照のショートのカットの上書きを全部外す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::OverrideCut;
use crate::touches::Touches;

/// 参照のショートのカット `cut` の in/out の上書きを外し、元のカットに戻す。逆操作は元の上書きを置く `overrideCut`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetCut {
    pub cut: CutId,
}

impl ResetCut {
    fn as_override(&self) -> OverrideCut {
        OverrideCut {
            cut: self.cut.clone(),
            in_point: None,
            out_point: None,
        }
    }
}

impl Apply for ResetCut {
    fn touches(&self, project: &Project) -> Result<Touches> {
        self.as_override().touches(project)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        self.as_override().set(project)
    }
}
