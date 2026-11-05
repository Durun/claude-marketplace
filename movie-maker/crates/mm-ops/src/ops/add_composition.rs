//! `addComposition`。コンポジションを足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Composition, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, layer_refs, RemoveComposition};
use crate::touches::Touches;

/// コンポジションを足す（レイヤーとマーカーを含めてよい）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddComposition {
    pub composition: Composition,
}

impl Apply for AddComposition {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.composition.id);
        for l in &self.composition.layers {
            t.add(&l.id);
            layer_refs(l, &mut t);
        }
        for m in &self.composition.markers {
            t.add(&m.id);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.composition(&self.composition.id).is_ok() {
            return Err(invalid(format!(
                "コンポジション {} は既に在る",
                self.composition.id
            )));
        }
        project.compositions.push(self.composition.clone());
        Ok(RemoveComposition {
            composition: self.composition.id.clone(),
        }
        .into())
    }
}
