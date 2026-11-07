//! `unprecompose`。入れ子のレイヤーを中のレイヤーに戻す（`precompose` の逆）。

use serde::{Deserialize, Serialize};

use mm_doc::{Composition, Layer, Node, Project, NESTED_KIND, NESTED_PARAM};
use mm_types::{CompositionId, Id, LayerId, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::{composition_referrers, invalid, Precompose};
use crate::touches::Touches;

/// 入れ子のレイヤー `layer` を外し、中のコンポジションのレイヤーをその位置に戻し、中のコンポジションを消す。
///
/// 逆操作が `precompose` になるよう、`precompose` が作る形（入れ子のレイヤーが既定の時間と重ね方で、
/// 中のコンポジションが親と同じ大きさ・fps・長さでマーカーが無く、ほかから参照されない）だけ戻せる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Unprecompose {
    pub layer: LayerId,
}

impl Unprecompose {
    fn nested_comp(&self, project: &Project) -> Result<CompositionId> {
        let (_, layer) = project.layer(&self.layer)?;
        let id = layer
            .source
            .text_param(NESTED_PARAM)
            .filter(|_| layer.source.kind == NESTED_KIND)
            .ok_or_else(|| invalid(format!("レイヤー {} は入れ子でない", self.layer)))?;
        Ok(Id::new(id))
    }
}

impl Apply for Unprecompose {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (comp_id, _) = project.layer_index(&self.layer)?;
        let nested_id = self.nested_comp(project)?;
        let mut t = Touches::of(&comp_id);
        t.add(&nested_id).add(&self.layer);
        composition_referrers(project, &nested_id, &mut t);
        for l in &project.composition(&nested_id)?.layers {
            t.add(&l.id);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (comp_id, index) = project.layer_index(&self.layer)?;
        let nested_id = self.nested_comp(project)?;
        check_shape(project, &comp_id, &nested_id, index)?;
        let pos = project
            .compositions
            .iter()
            .position(|c| c.id == nested_id)
            .expect("nested_comp が引いた");
        let nested = project.compositions.remove(pos);
        let comp = project.composition_mut(&comp_id)?;
        comp.layers.remove(index);
        let layers = nested.layers.iter().map(|l| l.id.clone()).collect();
        let tail = comp.layers.split_off(index);
        comp.layers.extend(nested.layers);
        comp.layers.extend(tail);
        Ok(Precompose {
            composition: comp_id,
            layers,
            new_composition: nested_id,
            new_layer: self.layer.clone(),
        }
        .into())
    }
}

/// `precompose` が作る形か。
fn check_shape(
    project: &Project,
    comp_id: &CompositionId,
    nested_id: &CompositionId,
    index: usize,
) -> Result<()> {
    let comp = project.composition(comp_id)?;
    let nested = project.composition(nested_id)?;
    check_layer_shape(comp, nested_id, index)?;
    let same_shape = nested.width == comp.width
        && nested.height == comp.height
        && nested.fps == comp.fps
        && nested.duration == comp.duration
        && nested.markers.is_empty()
        && nested.mixer.is_none()
        && !nested.layers.is_empty();
    if !same_shape {
        return Err(invalid(format!(
            "コンポジション {nested_id} は precompose の形でない"
        )));
    }
    check_unreferenced(project, &comp.layers[index].id, nested_id)
}

/// 入れ子のレイヤーが `precompose` の既定の時間と重ね方か。
fn check_layer_shape(comp: &Composition, nested_id: &CompositionId, index: usize) -> Result<()> {
    let layer = &comp.layers[index];
    let expected = Layer::new(
        layer.id.as_str(),
        Node::new(NESTED_KIND, layer.source.version).with_param(
            NESTED_PARAM,
            mm_types::ParamValue::Text(nested_id.as_str().to_owned()),
        ),
        RationalTime::ZERO,
        RationalTime::ZERO,
        comp.duration,
    );
    if *layer != expected {
        return Err(invalid(format!(
            "レイヤー {} は precompose の既定の形でない",
            layer.id
        )));
    }
    Ok(())
}

/// 中のコンポジションを `layer` のほかが参照していないか（効果・カット・トランジション・レンダーターゲットも見る）。
fn check_unreferenced(project: &Project, layer: &LayerId, nested_id: &CompositionId) -> Result<()> {
    let is_nested_ref = |n: &Node| {
        n.descendants().any(|n| {
            n.kind == NESTED_KIND && n.text_param(NESTED_PARAM) == Some(nested_id.as_str())
        })
    };
    let by_layer = project
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .filter(|l| &l.id != layer)
        .flat_map(Layer::nodes)
        .any(is_nested_ref);
    let by_cut = project
        .sequences
        .iter()
        .flat_map(|s| s.cuts.iter())
        .flat_map(mm_doc::Cut::nodes)
        .any(is_nested_ref);
    let targeted = project
        .render_targets
        .iter()
        .any(|t| t.composition == *nested_id);
    if by_layer || by_cut || targeted {
        return Err(invalid(format!(
            "コンポジション {nested_id} はほかから参照されている"
        )));
    }
    Ok(())
}
