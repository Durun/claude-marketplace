//! `rate`。選別か話題の評価を変える。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, Rating};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::bins::item_mut;
use crate::touches::Touches;

/// 選別か話題（`target` の ID）の評価を `rating` にする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    /// 選別か話題の ID。
    pub target: String,
    pub rating: Rating,
}

impl Apply for Rate {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::new();
        t.add_str(&self.target);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (rating, _) = item_mut(project, &self.target)?;
        let old = std::mem::replace(rating, self.rating);
        Ok(Rate {
            target: self.target.clone(),
            rating: old,
        }
        .into())
    }
}
