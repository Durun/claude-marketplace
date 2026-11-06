//! `removeRenderTarget`。レンダーターゲットを外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{RenderTargetId, Result};

use crate::op::{Apply, Op};
use crate::ops::AddRenderTarget;
use crate::touches::Touches;

/// レンダーターゲットを外す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveRenderTarget {
    pub target: RenderTargetId,
}

impl Apply for RemoveRenderTarget {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.target))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        project.render_target(&self.target)?;
        let pos = project
            .render_targets
            .iter()
            .position(|t| t.id == self.target)
            .expect("render_target が引いた");
        let target = project.render_targets.remove(pos);
        Ok(AddRenderTarget {
            target,
            index: Some(pos),
        }
        .into())
    }
}
