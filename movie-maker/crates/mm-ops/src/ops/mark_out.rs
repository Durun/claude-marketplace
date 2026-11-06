//! `markOut`。素材のアウト点の印を付ける・外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{AssetId, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::bins::set_mark;
use crate::ops::Edge;
use crate::touches::Touches;

/// 素材 `asset` のアウト点の印を `at`（素材の時間軸、含まない）にする。`None` で外す。イン点の印より後でなければ拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkOut {
    pub asset: AssetId,
    pub at: Option<RationalTime>,
}

impl Apply for MarkOut {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.asset))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let old = set_mark(project, &self.asset, Edge::Out, self.at)?;
        Ok(MarkOut {
            asset: self.asset.clone(),
            at: old,
        }
        .into())
    }
}
