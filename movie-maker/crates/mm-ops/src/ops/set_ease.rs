//! `setEase`。キーフレームの補間とイージングを変える。

use serde::{Deserialize, Serialize};

use mm_doc::{CubicBezier, Ease, Interpolation, Project};
use mm_types::{MmError, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::param_ref::ParamRef;
use crate::touches::Touches;

/// 時刻 `time` のキーフレームの区間の形を変える。値と時刻は変えない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetEase {
    pub target: ParamRef,
    pub time: RationalTime,
    pub interpolation: Interpolation,
    #[serde(default)]
    pub ease: Ease,
    /// `Bezier` の制御点。`None` は既定（ease-in-out）。
    #[serde(default)]
    pub bezier: Option<CubicBezier>,
}

impl Apply for SetEase {
    fn touches(&self, _: &Project) -> Result<Touches> {
        self.target.touches()
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let mut slot = self.target.resolve(project)?;
        let param = slot.get_mut()?;
        let k = param
            .keyframes
            .iter_mut()
            .find(|k| k.time == self.time)
            .ok_or_else(|| MmError::NotFound(format!("時刻 {} のキーフレーム", self.time)))?;
        let inverse = SetEase {
            target: self.target.clone(),
            time: self.time,
            interpolation: k.interpolation,
            ease: k.ease,
            bezier: k.bezier,
        };
        k.interpolation = self.interpolation;
        k.ease = self.ease;
        k.bezier = self.bezier;
        Ok(inverse.into())
    }
}
