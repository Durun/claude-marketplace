//! `parentLayer`。親のレイヤーを付ける・外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::touches::Touches;

/// 親を `parent` にする。`None` で外す。循環と無い親は `Project::validate` が断る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParentLayer {
    pub layer: LayerId,
    #[serde(default)]
    pub parent: Option<LayerId>,
}

impl Apply for ParentLayer {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.layer);
        if let Some(p) = &self.parent {
            t.add(p);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let layer = project.layer_mut(&self.layer)?;
        let parent = std::mem::replace(&mut layer.parent, self.parent.clone());
        Ok(ParentLayer {
            layer: self.layer.clone(),
            parent,
        }
        .into())
    }
}
