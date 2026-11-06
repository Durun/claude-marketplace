//! `disable`。カットを配置から外す・戻す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::cut_referrers;
use crate::touches::Touches;

/// `cut` の `enabled` を `!disabled` にする。外したカットは消えずに残り、配置から除かれる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Disable {
    pub cut: CutId,
    /// `true` で外し、`false` で戻す。
    #[serde(default = "default_disabled")]
    pub disabled: bool,
}

fn default_disabled() -> bool {
    true
}

impl Apply for Disable {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.cut);
        cut_referrers(project, &self.cut, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let cut = project.cut_mut(&self.cut)?;
        let was_disabled = !std::mem::replace(&mut cut.enabled, !self.disabled);
        Ok(Disable {
            cut: self.cut.clone(),
            disabled: was_disabled,
        }
        .into())
    }
}
