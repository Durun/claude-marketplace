//! `removeComposition`。コンポジションを外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{CompositionId, Result};

use crate::op::{Apply, Op};
use crate::ops::{composition_referrers, AddComposition};
use crate::touches::Touches;

/// コンポジションを外す。入れ子やレンダーターゲットから参照されていれば `Project::validate` が断る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveComposition {
    pub composition: CompositionId,
}

impl Apply for RemoveComposition {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let comp = project.composition(&self.composition)?;
        let mut t = Touches::of(&comp.id);
        for l in &comp.layers {
            t.add(&l.id);
        }
        for m in &comp.markers {
            t.add(&m.id);
        }
        composition_referrers(project, &comp.id, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        project.composition(&self.composition)?;
        let pos = project
            .compositions
            .iter()
            .position(|c| c.id == self.composition)
            .expect("composition が引いた");
        let composition = project.compositions.remove(pos);
        Ok(AddComposition { composition }.into())
    }
}
