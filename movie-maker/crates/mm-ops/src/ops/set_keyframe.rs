//! `setKeyframe`。キーフレームを置く。

use serde::{Deserialize, Serialize};

use mm_doc::{Keyframe, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::RemoveKeyframe;
use crate::param_ref::ParamRef;
use crate::touches::Touches;

/// `target` に `keyframe` を置く。同じ時刻のキーフレームがあれば置き換え、無ければ時刻の順に差し込む。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetKeyframe {
    pub target: ParamRef,
    pub keyframe: Keyframe,
}

impl Apply for SetKeyframe {
    fn touches(&self, _: &Project) -> Result<Touches> {
        self.target.touches()
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let mut slot = self.target.resolve(project)?;
        let param = slot.get_mut()?;
        let time = self.keyframe.time;
        let pos = param.keyframes.partition_point(|k| k.time < time);
        if let Some(existing) = param.keyframes.get_mut(pos).filter(|k| k.time == time) {
            let old = std::mem::replace(existing, self.keyframe.clone());
            return Ok(SetKeyframe {
                target: self.target.clone(),
                keyframe: old,
            }
            .into());
        }
        param.keyframes.insert(pos, self.keyframe.clone());
        Ok(RemoveKeyframe {
            target: self.target.clone(),
            time,
        }
        .into())
    }
}
