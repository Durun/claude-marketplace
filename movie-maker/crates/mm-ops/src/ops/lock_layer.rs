//! `lockLayer`。ロックを付ける・外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::touches::Touches;

/// ロックを `lock` にする。ロックの付いたレイヤーは Claude の操作経路から触れない（外すのも）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockLayer {
    pub layer: LayerId,
    pub lock: bool,
}

impl Apply for LockLayer {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.layer))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let layer = project.layer_mut(&self.layer)?;
        let lock = std::mem::replace(&mut layer.lock, self.lock);
        Ok(LockLayer {
            layer: self.layer.clone(),
            lock,
        }
        .into())
    }
}
