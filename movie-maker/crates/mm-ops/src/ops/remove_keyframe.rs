//! `removeKeyframe`。キーフレームを外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{MmError, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::SetKeyframe;
use crate::param_ref::ParamRef;
use crate::touches::Touches;

/// `target` の時刻 `time` のキーフレームを外す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveKeyframe {
    pub target: ParamRef,
    pub time: RationalTime,
}

impl Apply for RemoveKeyframe {
    fn touches(&self, _: &Project) -> Result<Touches> {
        self.target.touches()
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let mut slot = self.target.resolve(project)?;
        let param = slot.get_mut()?;
        let pos = param
            .keyframes
            .iter()
            .position(|k| k.time == self.time)
            .ok_or_else(|| MmError::NotFound(format!("時刻 {} のキーフレーム", self.time)))?;
        let keyframe = param.keyframes.remove(pos);
        Ok(SetKeyframe {
            target: self.target.clone(),
            keyframe,
        }
        .into())
    }
}
