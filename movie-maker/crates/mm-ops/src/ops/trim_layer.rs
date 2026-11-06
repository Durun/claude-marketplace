//! `trimLayer`。レイヤーのイン点かアウト点を動かす（Timeline の帯の端のつまみ）。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, FOOTAGE_KIND, FOOTAGE_PARAM};
use mm_types::{LayerId, ParamValue, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::{invalid, negate, node_refs, Edge};
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
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (_, layer) = project.layer(&self.layer)?;
        let mut t = Touches::of(&self.layer);
        // 撮影素材の長さを読んで範囲を検査するので、素材の参照も読む所に入れる。
        node_refs(&layer.source, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let duration = footage_duration(project, &self.layer)?;
        let layer = project.layer_mut(&self.layer)?;
        let delta = self.by.scale(layer.speed)?;
        let (mut start, mut in_point, mut out_point) =
            (layer.start, layer.in_point, layer.out_point);
        match self.edge {
            Edge::In => {
                if layer.time_remap.is_some() {
                    return Err(invalid(format!(
                        "レイヤー {} はタイムリマップを持つのでイン点を動かせない",
                        self.layer
                    )));
                }
                start = start.checked_add(self.by)?;
                in_point = in_point.checked_add(delta)?;
            }
            Edge::Out => out_point = out_point.checked_add(delta)?,
        }
        if let Some(duration) = duration {
            if in_point < RationalTime::ZERO || out_point > duration {
                return Err(invalid(format!(
                    "レイヤー {} が素材の外（0 から {} 秒）にはみ出す",
                    self.layer,
                    duration.to_f64()
                )));
            }
        }
        (layer.start, layer.in_point, layer.out_point) = (start, in_point, out_point);
        Ok(TrimLayer {
            layer: self.layer.clone(),
            edge: self.edge,
            by: negate(self.by)?,
        }
        .into())
    }
}

/// 撮影素材のレイヤーの素材の長さ。撮影素材でなければ `None`（範囲は縛らない）。
fn footage_duration(project: &Project, layer: &LayerId) -> Result<Option<RationalTime>> {
    let (_, layer) = project.layer(layer)?;
    if layer.source.kind != FOOTAGE_KIND {
        return Ok(None);
    }
    match layer.source.params.get(FOOTAGE_PARAM).map(|p| &p.value) {
        Some(ParamValue::AssetRef(id)) => Ok(Some(project.asset(id)?.duration)),
        _ => Ok(None),
    }
}
