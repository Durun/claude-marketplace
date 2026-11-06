//! `parentLayer`。親のレイヤーを付ける・外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::ops::add_ancestors;
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
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (comp, layer) = project.layer(&self.layer)?;
        let mut t = Touches::of(&self.layer);
        // 循環の判定は新しい親の祖先の鎖を読むので、鎖の全部を持つ。前の親は逆操作が戻す先。
        for parent in self.parent.iter().chain(layer.parent.iter()) {
            t.add(parent);
            add_ancestors(comp, parent, &mut t);
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
