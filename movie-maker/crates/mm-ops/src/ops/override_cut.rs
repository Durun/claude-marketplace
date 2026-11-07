//! `overrideCut`。参照のショートのカットの in/out を上書きする・上書きを外す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, CutOverrides, Project};
use mm_types::{RationalTime, Result};

use crate::op::{Apply, Op};
use crate::shorts::reference_pair;
use crate::touches::Touches;

/// 参照のショートのカット `cut` の上書きを置く。値のある所はその値で上書きし（フラグが立つ）、省いた所は上書きを外して
/// 元のカットに従う。上書きしていない所は元のカットを直すと一緒に変わる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverrideCut {
    pub cut: CutId,
    /// 上書きするイン点（素材の時間軸）。
    #[serde(rename = "in", default, skip_serializing_if = "Option::is_none")]
    pub in_point: Option<RationalTime>,
    /// 上書きするアウト点（素材の時間軸、含まない）。
    #[serde(rename = "out", default, skip_serializing_if = "Option::is_none")]
    pub out_point: Option<RationalTime>,
}

impl OverrideCut {
    /// カットの今の上書きをそのまま置く操作（逆操作に使う）。
    pub(crate) fn current(project: &Project, id: &CutId) -> Result<Self> {
        let (cut, _) = reference_pair(project, id)?;
        Ok(Self {
            cut: id.clone(),
            in_point: cut.overrides.in_point.then_some(cut.in_point),
            out_point: cut.overrides.out_point.then_some(cut.out_point),
        })
    }

    /// 上書きを置いて逆操作を返す。
    pub(crate) fn set(&self, project: &mut Project) -> Result<Op> {
        let inverse = Self::current(project, &self.cut)?;
        let main = reference_pair(project, &self.cut)?.1.clone();
        let cut = project.cut_mut(&self.cut)?;
        cut.overrides = CutOverrides {
            in_point: self.in_point.is_some(),
            out_point: self.out_point.is_some(),
        };
        cut.in_point = self.in_point.unwrap_or(main.in_point);
        cut.out_point = self.out_point.unwrap_or(main.out_point);
        Ok(inverse.into())
    }
}

impl Apply for OverrideCut {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (cut, main) = reference_pair(project, &self.cut)?;
        let mut t = Touches::of(&cut.id);
        t.add(&main.id);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        self.set(project)
    }
}
