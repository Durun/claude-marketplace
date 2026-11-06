//! `fitLayer`。sequence を置いたレイヤーの長さとコンポジションの長さを、sequence の長さに合わせる。

use serde::{Deserialize, Serialize};

use mm_doc::{Composition, Layer, Project, Sequence, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_types::{Id, LayerId, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::invalid;
use crate::touches::Touches;

/// sequence を置いたレイヤー `layer` のアウト点を、sequence の長さ（`insertTopics` や `insert` で伸び縮みする）に合わせる。
/// コンポジションの長さは、これまでレイヤーの終わりと同じだった（または新しい終わりが超える）ときだけ、
/// 全レイヤーの終わりの最大に合わせる。長さを自分で決めたコンポジションは縮めない。
/// `out_point` と `duration` を渡すとその値を置く（逆操作の形。人は省く）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FitLayer {
    pub layer: LayerId,
    #[serde(default)]
    pub out_point: Option<RationalTime>,
    #[serde(default)]
    pub duration: Option<RationalTime>,
}

/// コンポジションの時間軸でのレイヤーの終わり。
fn end_of(layer: &Layer) -> Result<RationalTime> {
    layer.start.checked_add(layer.duration()?)
}

/// 全レイヤーの終わりの最大。レイヤーが無ければ 0。
fn latest_end(comp: &Composition) -> Result<RationalTime> {
    let mut latest = RationalTime::ZERO;
    for layer in &comp.layers {
        latest = latest.max(end_of(layer)?);
    }
    Ok(latest)
}

/// sequence の長さ（有効なカットの長さの和。トランジションは配置を変えない）。
fn sequence_length(seq: &Sequence) -> Result<RationalTime> {
    seq.cuts
        .iter()
        .filter(|c| c.enabled)
        .try_fold(RationalTime::ZERO, |acc, c| acc.checked_add(c.duration()?))
}

/// 長さが変わりうる操作 `ops` の後ろに、sequence の長さに合わせている（レイヤーの長さが操作の前の sequence の長さと同じ）
/// レイヤーの `fitLayer` を足す。1 つのトランザクションにするので、取り消し 1 回でレイヤーの長さも戻る。
/// 自分で縮めたレイヤーや、既に `fitLayer` を含む列には足さない。
pub fn with_fitted_layers(project: &Project, ops: &[Op]) -> Vec<Op> {
    let mut touched = Touches::new();
    for op in ops {
        if let Ok(t) = op.touches(project) {
            touched.extend(&t);
        }
    }
    let explicit = |id: &LayerId| {
        ops.iter()
            .any(|o| matches!(o, Op::FitLayer(f) if &f.layer == id))
    };
    let mut out = ops.to_vec();
    for seq in &project.sequences {
        let mut mine = Touches::of(&seq.id);
        for cut in &seq.cuts {
            mine.add(&cut.id);
        }
        if !mine.overlaps(&touched) {
            continue;
        }
        let Ok(length) = sequence_length(seq) else {
            continue;
        };
        let layers = project.compositions.iter().flat_map(|c| &c.layers);
        for layer in layers {
            let placed = layer.source.kind == SEQUENCE_KIND
                && layer.source.text_param(SEQUENCE_PARAM) == Some(seq.id.as_str());
            if placed && layer.duration().is_ok_and(|d| d == length) && !explicit(&layer.id) {
                out.push(
                    FitLayer {
                        layer: layer.id.clone(),
                        out_point: None,
                        duration: None,
                    }
                    .into(),
                );
            }
        }
    }
    out
}

impl FitLayer {
    /// sequence の長さに合うアウト点。
    fn fitted_out(&self, project: &Project, layer: &Layer) -> Result<RationalTime> {
        if layer.source.kind != SEQUENCE_KIND {
            return Err(invalid(format!(
                "レイヤー {} は sequence を置いていない",
                self.layer
            )));
        }
        let id = Id::new(layer.source.text_param(SEQUENCE_PARAM).unwrap_or(""));
        let length = sequence_length(project.sequence(&id)?)?;
        layer.in_point.checked_add(length.scale(layer.speed)?)
    }
}

impl Apply for FitLayer {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (comp, layer) = project.layer(&self.layer)?;
        let mut t = Touches::of(&self.layer);
        t.add(&comp.id);
        // コンポジションの長さは全レイヤーの終わりから決まる。
        for other in &comp.layers {
            t.add(&other.id);
        }
        // アウト点は sequence の全カットの長さから決まる。
        if layer.source.kind == SEQUENCE_KIND {
            let id = Id::new(layer.source.text_param(SEQUENCE_PARAM).unwrap_or(""));
            if let Ok(seq) = project.sequence(&id) {
                t.add(&seq.id);
                for cut in &seq.cuts {
                    t.add(&cut.id);
                }
            }
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (comp, layer) = project.layer(&self.layer)?;
        let comp_id = comp.id.clone();
        let old_out = layer.out_point;
        let old_duration = comp.duration;
        let old_end = end_of(layer)?;
        let out = match self.out_point {
            Some(out) => out,
            None => self.fitted_out(project, layer)?,
        };

        let layer = project.layer_mut(&self.layer)?;
        layer.out_point = out;
        let new_end = end_of(layer)?;
        let comp = project.composition_mut(&comp_id)?;
        let follows = old_duration == old_end || new_end > old_duration;
        comp.duration = match self.duration {
            Some(d) => d,
            None if follows => latest_end(comp)?,
            None => old_duration,
        };
        Ok(FitLayer {
            layer: self.layer.clone(),
            out_point: Some(old_out),
            duration: Some(old_duration),
        }
        .into())
    }
}
