//! `addAsset`。素材を足す。取り込み（`mm-ingest`）がコンテンツハッシュを付けて呼ぶ。

use serde::{Deserialize, Serialize};

use mm_doc::{Asset, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveAsset};
use crate::touches::Touches;

/// 素材を足す。同じ ID の素材が既にあれば拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddAsset {
    pub asset: Asset,
}

impl Apply for AddAsset {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.asset.id))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.asset(&self.asset.id).is_ok() {
            return Err(invalid(format!("素材 {} は既に在る", self.asset.id)));
        }
        project.assets.push(self.asset.clone());
        Ok(RemoveAsset {
            asset: self.asset.id.clone(),
        }
        .into())
    }
}
