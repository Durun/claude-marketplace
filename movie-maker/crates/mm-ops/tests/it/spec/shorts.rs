//! 本編とショートの操作の種（上書き、再フレーミング、字幕の置き場所）。

use super::build::{cut_id, layer_id, pick};
use crate::common::quarters;
use mm_doc::{CaptionPlacement, OriginRelation, Param, Project, Transform, SEQUENCE_PARAM};
use mm_ops::*;
use mm_types::ParamValue;
use proptest::prelude::*;

/// 本編とショートの操作の種。
#[derive(Debug, Clone)]
pub enum ShortSpec {
    /// 参照のカットの番号、in と out の上書き（元からの 1/4 秒単位のずれ）。
    Override(u8, Option<i64>, Option<i64>),
    Reset(u8),
    /// sequence のレイヤーの番号、カットの番号、拡大（`None` で外す）。
    Reframe(u8, u8, Option<i64>),
    /// レイヤーの番号、字幕の幅（`None` で外す）。
    Caption(u8, Option<i64>),
}

pub fn arb_short_spec() -> impl Strategy<Value = ShortSpec> {
    let s = || any::<u8>();
    let shift = || prop::option::of(-4i64..4);
    prop_oneof![
        (s(), shift(), shift()).prop_map(|(a, b, c)| ShortSpec::Override(a, b, c)),
        s().prop_map(ShortSpec::Reset),
        (s(), s(), prop::option::of(1i64..8)).prop_map(|(a, b, c)| ShortSpec::Reframe(a, b, c)),
        (s(), prop::option::of(1i64..8)).prop_map(|(a, b)| ShortSpec::Caption(a, b)),
    ]
}

/// 参照のショートの、元のあるカット。
fn reference_cuts(p: &Project) -> Vec<&mm_doc::Cut> {
    p.sequences
        .iter()
        .filter(|s| {
            s.origin
                .as_ref()
                .is_some_and(|o| o.relation == OriginRelation::Reference)
        })
        .flat_map(|s| s.cuts.iter())
        .filter(|c| c.origin.is_some())
        .collect()
}

fn number(n: i64) -> Param {
    Param::constant(ParamValue::Number(n as f64))
}

pub fn short_op(spec: &ShortSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        ShortSpec::Override(i, a, b) => {
            let cuts = reference_cuts(p);
            let cut = pick(&cuts, *i)?;
            let shift = |d: &Option<i64>, t| d.and_then(|d| quarters(d).checked_add(t).ok());
            OverrideCut {
                cut: cut.id.clone(),
                in_point: shift(a, cut.in_point),
                out_point: shift(b, cut.out_point),
            }
            .into()
        }
        ShortSpec::Reset(i) => ResetCut {
            cut: pick(&reference_cuts(p), *i)?.id.clone(),
        }
        .into(),
        ShortSpec::Reframe(l, c, scale) => {
            let layer = p
                .compositions
                .iter()
                .flat_map(|c| c.layers.iter())
                .filter(|l| l.source.text_param(SEQUENCE_PARAM).is_some())
                .collect::<Vec<_>>();
            let layer = pick(&layer, *l)?;
            let seq = p
                .sequences
                .iter()
                .find(|s| Some(s.id.as_str()) == layer.source.text_param(SEQUENCE_PARAM));
            let cut = seq
                .and_then(|s| pick(&s.cuts, *c))
                .map(|c| c.id.clone())
                .or_else(|| cut_id(p, *c))?;
            SetReframe {
                layer: layer.id.clone(),
                cut,
                transform: scale.map(|n| Transform {
                    scale: number(n),
                    ..Transform::default()
                }),
            }
            .into()
        }
        ShortSpec::Caption(l, width) => SetCaption {
            layer: layer_id(p, *l)?,
            caption: width.map(|w| CaptionPlacement {
                position: Param::constant(ParamValue::Vec2([0.0, w as f64])),
                width: number(w),
            }),
        }
        .into(),
    })
}
