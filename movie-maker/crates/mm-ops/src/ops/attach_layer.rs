//! `attachLayer`。カットへの接続を付ける・外す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project};
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::touches::Touches;

/// 接続先を `cut` にする。`None` で外す。無いカットは `Project::validate` が断る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachLayer {
    pub layer: LayerId,
    #[serde(default)]
    pub cut: Option<CutId>,
}

impl Apply for AttachLayer {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.layer);
        if let Some(c) = &self.cut {
            t.add(c);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let layer = project.layer_mut(&self.layer)?;
        let cut = std::mem::replace(&mut layer.attach_to, self.cut.clone());
        Ok(AttachLayer {
            layer: self.layer.clone(),
            cut,
        }
        .into())
    }
}
