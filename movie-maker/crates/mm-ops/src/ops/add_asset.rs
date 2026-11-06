//! `addAsset`。素材を足す。取り込み（`mm-ingest`）がコンテンツハッシュを付けて呼ぶ。

use serde::{Deserialize, Serialize};

use mm_doc::{Asset, Project};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveAsset};
use crate::touches::Touches;

/// 素材を足す。同じ ID の素材が既にあれば拒否する。`index` は素材の並びの位置で、無ければ末尾。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddAsset {
    pub asset: Asset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

impl Apply for AddAsset {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.asset.id))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.asset(&self.asset.id).is_ok() {
            return Err(invalid(format!("素材 {} は既に在る", self.asset.id)));
        }
        let index = self.index.unwrap_or(project.assets.len());
        if index > project.assets.len() {
            return Err(invalid(format!(
                "素材の位置 {index} が {} を超える",
                project.assets.len()
            )));
        }
        project.assets.insert(index, self.asset.clone());
        Ok(RemoveAsset {
            asset: self.asset.id.clone(),
        }
        .into())
    }
}
