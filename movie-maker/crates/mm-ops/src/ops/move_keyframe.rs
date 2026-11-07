//! `moveKeyframe`。キーフレームの時刻と中身を 1 つの操作で置き直す。

use serde::{Deserialize, Serialize};

use mm_doc::{Keyframe, Project};
use mm_types::{MmError, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::param_ref::ParamRef;
use crate::touches::Touches;

/// `target` の時刻 `from` のキーフレームを外し、`keyframe` を時刻の順の位置に置く。
///
/// Graph のドラッグ（時刻と値を同時に動かす）を取り消し 1 回で戻すため、外すと置くを 1 つにまとめる。
/// 動かした先に別のキーフレームがあれば拒否する（黙って上書きすると、取り消しで戻せないため）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveKeyframe {
    pub target: ParamRef,
    /// 動かすキーフレームの今の時刻。
    pub from: RationalTime,
    /// 置き直した後のキーフレーム（時刻、値、区間の形）。
    pub keyframe: Keyframe,
}

impl Apply for MoveKeyframe {
    fn touches(&self, _: &Project) -> Result<Touches> {
        self.target.touches()
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let mut slot = self.target.resolve(project)?;
        let param = slot.get_mut()?;
        let pos = param
            .keyframes
            .iter()
            .position(|k| k.time == self.from)
            .ok_or_else(|| MmError::NotFound(format!("時刻 {} のキーフレーム", self.from)))?;
        let to = self.keyframe.time;
        if to != self.from && param.keyframes.iter().any(|k| k.time == to) {
            return Err(MmError::InvalidArgument(format!(
                "時刻 {to} には別のキーフレームがある"
            )));
        }
        let old = param.keyframes.remove(pos);
        let at = param.keyframes.partition_point(|k| k.time < to);
        param.keyframes.insert(at, self.keyframe.clone());
        Ok(MoveKeyframe {
            target: self.target.clone(),
            from: to,
            keyframe: old,
        }
        .into())
    }
}
