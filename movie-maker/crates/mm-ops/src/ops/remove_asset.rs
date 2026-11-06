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
        Ok(AddAsset {
            asset,
            index: Some(pos),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::OpContext;
    use mm_doc::Asset;
    use mm_types::{ColorSpace, Id, RationalTime};

    fn asset(id: &str) -> Asset {
        Asset {
            id: Id::new(id),
            path: format!("{id}.mp4"),
            content_hash: format!("hash-{id}"),
            color_space: ColorSpace::bt709(),
            duration: RationalTime::seconds(1),
            fps: None,
            timebase_offset: RationalTime::ZERO,
        }
    }

    fn ids(p: &Project) -> Vec<&str> {
        p.assets.iter().map(|a| a.id.as_str()).collect()
    }

    #[test]
    fn 外した素材は逆操作で元の位置に戻る() {
        let mut p = Project::new();
        p.assets = vec![asset("z"), asset("a"), asset("m")];
        let remove: Op = RemoveAsset {
            asset: Id::new("a"),
        }
        .into();
        let applied = remove.apply(&mut p, &OpContext::human()).unwrap();
        assert_eq!(ids(&p), ["z", "m"]);
        applied.inverse.apply(&mut p, &OpContext::human()).unwrap();
        assert_eq!(ids(&p), ["z", "a", "m"]);
    }

    #[test]
    fn 位置が末尾を超える_add_asset_は拒否する() {
        let mut p = Project::new();
        let add = AddAsset {
            asset: asset("a"),
            index: Some(1),
        };
        assert!(add.apply(&mut p).is_err());
        assert!(p.assets.is_empty());
    }

    #[test]
    fn 位置を省いた_add_asset_は末尾に足す() {
        let mut p = Project::new();
        p.assets = vec![asset("z")];
        let add = AddAsset {
            asset: asset("a"),
            index: None,
        };
        add.apply(&mut p).unwrap();
        assert_eq!(ids(&p), ["z", "a"]);
    }
}
