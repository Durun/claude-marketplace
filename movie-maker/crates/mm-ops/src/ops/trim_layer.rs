//! `trimLayer`。レイヤーのイン点かアウト点を動かす（Timeline の帯の端のつまみ）。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::ops::{invalid, negate, Edge};
use crate::touches::Touches;

/// レイヤー `layer` の `edge` を、コンポジションの時間軸で `by` だけ動かす（素材の時間では `by × speed`）。
/// イン点は帯の左端と素材の位置を一緒に動かし（`start` と `in`）、画は動かない。アウト点は `out` だけ。
/// 正に動かすとイン点は短く、アウト点は長くなる。タイムリマップのあるレイヤーのイン点は拒否する（写像の起点が動くため）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrimLayer {
    pub layer: LayerId,
    pub edge: Edge,
    pub by: mm_types::RationalTime,
}

impl Apply for TrimLayer {
    fn touches(&self, _: &Project) -> Result<Touches> {
        Ok(Touches::of(&self.layer))
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let layer = project.layer_mut(&self.layer)?;
        let delta = self.by.scale(layer.speed)?;
        match self.edge {
            Edge::In => {
                if layer.time_remap.is_some() {
                    return Err(invalid(format!(
                        "レイヤー {} はタイムリマップを持つのでイン点を動かせない",
                        self.layer
                    )));
                }
                layer.start = layer.start.checked_add(self.by)?;
                layer.in_point = layer.in_point.checked_add(delta)?;
            }
            Edge::Out => layer.out_point = layer.out_point.checked_add(delta)?,
        }
        Ok(TrimLayer {
            layer: self.layer.clone(),
            edge: self.edge,
            by: negate(self.by)?,
        }
        .into())
    }
}
