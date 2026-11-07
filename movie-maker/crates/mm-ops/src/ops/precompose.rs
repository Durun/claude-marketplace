//! `precompose`。レイヤーの組を新しいコンポジションに入れ、入れ子のレイヤーで置き換える。

use serde::{Deserialize, Serialize};

use mm_doc::{Composition, Layer, Node, Project, NESTED_KIND, NESTED_PARAM};
use mm_types::{CompositionId, LayerId, ParamValue, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::{invalid, Unprecompose};
use crate::touches::Touches;

/// 入れ子のノードの種類のバージョン。
const NESTED_VERSION: u32 = 1;

/// `layers`（同じコンポジションの中で、重なりの上で続いている範囲）を新しいコンポジション
/// `new_composition` に移し、その位置に、それを参照するレイヤー `new_layer` を置く。
/// 新しいコンポジションは元と同じ大きさ・fps・長さ。移すレイヤーの `start` はそのまま。
/// 続いていない範囲は拒否する（逆操作の `unprecompose` が重なりの順を戻せないため）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Precompose {
    pub composition: CompositionId,
    pub layers: Vec<LayerId>,
    pub new_composition: CompositionId,
    pub new_layer: LayerId,
}

/// `ids` が `comp` の重なりの上で続いた範囲なら、その始まりの位置。
fn contiguous_range(comp: &Composition, ids: &[LayerId]) -> Option<usize> {
    let mut positions: Vec<usize> = ids
        .iter()
        .map(|id| comp.layers.iter().position(|l| &l.id == id))
        .collect::<Option<_>>()?;
    positions.sort_unstable();
    positions.dedup();
    let first = *positions.first()?;
    let contiguous =
        positions.len() == ids.len() && positions.iter().enumerate().all(|(i, p)| *p == first + i);
    contiguous.then_some(first)
}

impl Apply for Precompose {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.composition);
        t.add(&self.new_composition).add(&self.new_layer);
        for l in &self.layers {
            t.add(l);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if self.layers.is_empty() {
            return Err(invalid("precompose するレイヤーが無い"));
        }
        if project.composition(&self.new_composition).is_ok() {
            return Err(invalid(format!(
                "コンポジション {} は既に在る",
                self.new_composition
            )));
        }
        let comp = project.composition_mut(&self.composition)?;
        let index = contiguous_range(comp, &self.layers).ok_or_else(|| {
            invalid(format!(
                "レイヤーの一部がコンポジション {} に無いか、重なりの上で続いていない",
                self.composition
            ))
        })?;
        let moved: Vec<Layer> = comp
            .layers
            .drain(index..index + self.layers.len())
            .collect();
        let nested = Node::new(NESTED_KIND, NESTED_VERSION).with_param(
            NESTED_PARAM,
            ParamValue::Text(self.new_composition.as_str().to_owned()),
        );
        let new_layer = Layer::new(
            self.new_layer.as_str(),
            nested,
            RationalTime::ZERO,
            RationalTime::ZERO,
            comp.duration,
        );
        comp.layers.insert(index, new_layer);
        let new_comp = Composition::new(
            self.new_composition.clone(),
            comp.width,
            comp.height,
            comp.fps,
            comp.duration,
        )
        .with_layers(moved);
        project.compositions.push(new_comp);
        Ok(Unprecompose {
            layer: self.new_layer.clone(),
        }
        .into())
    }
}
