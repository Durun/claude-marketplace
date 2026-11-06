//! `extract`。カットを抜く（後ろが詰まる）。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_referrers, cuts_beside, Insert};
use crate::touches::Touches;

/// カットをシーケンスから抜く。接続しているレイヤーがあれば `Project::validate` が拒否する（先に外す）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Extract {
    pub cut: CutId,
}

impl Apply for Extract {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (seq_id, index) = project.cut_index(&self.cut)?;
        let mut t = Touches::of(&seq_id);
        t.add(&self.cut);
        cuts_beside(project.sequence(&seq_id)?, index, &mut t);
        cut_referrers(project, &self.cut, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (seq_id, index) = project.cut_index(&self.cut)?;
        let cut = project.sequence_mut(&seq_id)?.cuts.remove(index);
        Ok(Insert {
            sequence: seq_id,
            index,
            cut,
        }
        .into())
    }
}
