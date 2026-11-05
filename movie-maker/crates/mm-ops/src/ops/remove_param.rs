//! `removeParam`。無くてもよいパラメータを外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::SetParam;
use crate::param_ref::ParamRef;
use crate::touches::Touches;

/// `target` を外す。種類のパラメータと `transform.anchor` だけ外せる（必ず在る欄は断る）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParam {
    pub target: ParamRef,
}

impl Apply for RemoveParam {
    fn touches(&self, _: &Project) -> Result<Touches> {
        self.target.touches()
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let old = self.target.resolve(project)?.remove()?;
        Ok(SetParam {
            target: self.target.clone(),
            value: old.value,
            keyframes: Some(old.keyframes),
        }
        .into())
    }
}
