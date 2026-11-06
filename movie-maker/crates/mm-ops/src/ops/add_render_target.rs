//! `addRenderTarget`。レンダーターゲットを足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, RenderTarget};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveRenderTarget};
use crate::touches::Touches;

/// レンダーターゲットを足す。無いコンポジションは `Project::validate` が拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddRenderTarget {
    pub target: RenderTarget,
}

impl Apply for AddRenderTarget {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.target.id);
        t.add(&self.target.composition);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.render_target(&self.target.id).is_ok() {
            return Err(invalid(format!(
                "レンダーターゲット {} は既に在る",
                self.target.id
            )));
        }
        project.render_targets.push(self.target.clone());
        Ok(RemoveRenderTarget {
            target: self.target.id.clone(),
        }
        .into())
    }
}
