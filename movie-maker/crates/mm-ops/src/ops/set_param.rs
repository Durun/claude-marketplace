//! `setParam`。パラメータの固定の値を置く。

use serde::{Deserialize, Serialize};

use mm_doc::{Keyframe, Param, Project};
use mm_types::{ParamValue, Result};

use crate::op::{Apply, Op};
use crate::ops::RemoveParam;
use crate::param_ref::ParamRef;
use crate::touches::Touches;

/// `target` の固定の値を `value` にする。
///
/// `keyframes` を省けばキーフレームは保つ。渡せばキーフレームの列も置き換える（空で消す）。
/// 無くてもよいフィールド（ノードのパラメータ、`transform.anchor`）が無ければ作る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParam {
    pub target: ParamRef,
    pub value: ParamValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyframes: Option<Vec<Keyframe>>,
}

impl Apply for SetParam {
    fn touches(&self, project: &Project) -> Result<Touches> {
        self.target.touches_in(project, Some(&self.value))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let mut slot = self.target.resolve(project)?;
        let Some(old) = slot.get().cloned() else {
            slot.set(Param {
                value: self.value.clone(),
                keyframes: self.keyframes.clone().unwrap_or_default(),
            });
            return Ok(RemoveParam {
                target: self.target.clone(),
            }
            .into());
        };
        let param = slot.get_mut()?;
        param.value = self.value.clone();
        if let Some(k) = &self.keyframes {
            param.keyframes = k.clone();
        }
        Ok(SetParam {
            target: self.target.clone(),
            value: old.value,
            keyframes: self.keyframes.as_ref().map(|_| old.keyframes),
        }
        .into())
    }
}
