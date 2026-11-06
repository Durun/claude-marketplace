//! `removeAsset`。素材を外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{AssetId, Result};

use crate::op::{Apply, Op};
use crate::ops::AddAsset;
use crate::touches::Touches;

/// 素材を外す。まだ参照しているノードがあれば、適用の後の検証が拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveAsset {
    pub asset: AssetId,
}

impl Apply for RemoveAsset {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.asset))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        project.asset(&self.asset)?;
        let pos = project
            .assets
            .iter()
            .position(|a| a.id == self.asset)
            .expect("asset が引いた");
        let asset = project.assets.remove(pos);
        Ok(AddAsset { asset }.into())
    }
}
