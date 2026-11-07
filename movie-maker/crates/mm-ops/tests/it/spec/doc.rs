//! プロジェクトのデータを編集する操作（レイヤー、パラメータ、コンポジション、レンダーターゲット、アセット、マーカー）の種。

use super::build::{arb_id, arb_layer, build_layer, cut_id, layer_id, pick, LayerSpec};
use super::params::{param_ref, value_for};
use crate::common::{marker, secs};
use mm_doc::{Ease, Interpolation, Keyframe, Project};
use mm_ops::*;
use mm_types::{Id, ParamValue};
use proptest::prelude::*;

/// プロジェクトのデータを編集する操作の種。
#[derive(Debug, Clone)]
pub enum DocSpec {
    AddLayer {
        comp: u8,
        at: u8,
        start: i64,
        len: i64,
        layer: LayerSpec,
    },
    RemoveLayer(u8),
    MoveLayer(u8, u8),
    SetParam(u8, u8, i64),
    RemoveParam(u8, u8),
    SetKeyframe(u8, u8, i64, i64),
    RemoveKeyframe(u8, u8, u8),
    SetEase(u8, u8, u8, u8),
    /// レイヤー、パス、元の時刻、先の時刻、値。
    MoveKeyframe(u8, u8, u8, i64, i64),
    ParentLayer(u8, u8),
    AttachLayer(u8, u8),
    LockLayer(u8, bool),
    Precompose {
        comp: u8,
        mask: u8,
        new_comp: String,
        new_layer: String,
    },
    Unprecompose(u8),
    AddComposition(String, Vec<LayerSpec>),
    RemoveComposition(u8),
    AddRenderTarget(u8, String),
    RemoveRenderTarget(u8),
    AddAsset(String),
    RemoveAsset(u8),
    AddMarker(u8, String, i64),
    RemoveMarker(u8),
}

pub fn arb() -> impl Strategy<Value = DocSpec> {
    let s = || any::<u8>();
    let t = || -6i64..6;
    prop_oneof![
        (s(), s(), 0i64..5, 1i64..5, arb_layer()).prop_map(|(comp, at, start, len, layer)| {
            DocSpec::AddLayer {
                comp,
                at,
                start,
                len,
                layer,
            }
        }),
        s().prop_map(DocSpec::RemoveLayer),
        (s(), s()).prop_map(|(a, b)| DocSpec::MoveLayer(a, b)),
        (s(), s(), t()).prop_map(|(a, b, c)| DocSpec::SetParam(a, b, c)),
        (s(), s()).prop_map(|(a, b)| DocSpec::RemoveParam(a, b)),
        (s(), s(), 0i64..4, t()).prop_map(|(a, b, c, d)| DocSpec::SetKeyframe(a, b, c, d)),
        (s(), s(), s()).prop_map(|(a, b, c)| DocSpec::RemoveKeyframe(a, b, c)),
        (s(), s(), s(), s()).prop_map(|(a, b, c, d)| DocSpec::SetEase(a, b, c, d)),
        (s(), s(), s(), 0i64..4, t())
            .prop_map(|(a, b, c, d, e)| DocSpec::MoveKeyframe(a, b, c, d, e)),
        (s(), s()).prop_map(|(a, b)| DocSpec::ParentLayer(a, b)),
        (s(), s()).prop_map(|(a, b)| DocSpec::AttachLayer(a, b)),
        (s(), any::<bool>()).prop_map(|(a, b)| DocSpec::LockLayer(a, b)),
        (s(), 1u8..16, arb_id(), arb_id()).prop_map(|(comp, mask, new_comp, new_layer)| {
            DocSpec::Precompose {
                comp,
                mask,
                new_comp,
                new_layer,
            }
        }),
        s().prop_map(DocSpec::Unprecompose),
        (arb_id(), prop::collection::vec(arb_layer(), 0..3))
            .prop_map(|(id, layers)| DocSpec::AddComposition(id, layers)),
        s().prop_map(DocSpec::RemoveComposition),
        (s(), arb_id()).prop_map(|(a, b)| DocSpec::AddRenderTarget(a, b)),
        s().prop_map(DocSpec::RemoveRenderTarget),
        arb_id().prop_map(DocSpec::AddAsset),
        s().prop_map(DocSpec::RemoveAsset),
        (s(), arb_id(), 0i64..10).prop_map(|(a, b, c)| DocSpec::AddMarker(a, b, c)),
        s().prop_map(DocSpec::RemoveMarker),
    ]
}

/// 種を今のプロジェクトのデータの ID に当てはめる。当てはめる相手が無ければ `None`。
pub fn op(spec: &DocSpec, p: &Project) -> Option<Op> {
    layer_op(spec, p)
        .or_else(|| param_op(spec, p))
        .or_else(|| comp_op(spec, p))
}

fn layer_op(spec: &DocSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        DocSpec::AddLayer {
            comp,
            at,
            start,
            len,
            layer,
        } => {
            let c = pick(&p.compositions, *comp)?;
            AddLayer {
                composition: c.id.clone(),
                layer: build_layer(layer, p, *start, *len),
                index: Some(usize::from(*at) % (c.layers.len() + 1)),
            }
            .into()
        }
        DocSpec::RemoveLayer(i) => RemoveLayer {
            layer: layer_id(p, *i)?,
        }
        .into(),
        DocSpec::MoveLayer(i, j) => MoveLayer {
            layer: layer_id(p, *i)?,
            index: usize::from(*j) % 3,
        }
        .into(),
        DocSpec::ParentLayer(i, j) => ParentLayer {
            layer: layer_id(p, *i)?,
            parent: if *j % 4 == 0 { None } else { layer_id(p, *j) },
        }
        .into(),
        DocSpec::AttachLayer(i, j) => AttachLayer {
            layer: layer_id(p, *i)?,
            cut: if *j % 4 == 0 { None } else { cut_id(p, *j) },
        }
        .into(),
        DocSpec::LockLayer(i, lock) => LockLayer {
            layer: layer_id(p, *i)?,
            lock: *lock,
        }
        .into(),
        _ => return None,
    })
}

fn param_op(spec: &DocSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        DocSpec::SetParam(i, path, v) => {
            let target = param_ref(p, *i, *path, false)?;
            let value = value_for(p, &target.path, *v);
            let keyframes = (*v % 3 == 0).then(Vec::new);
            SetParam {
                target,
                value,
                keyframes,
            }
            .into()
        }
        DocSpec::RemoveParam(i, path) => RemoveParam {
            target: param_ref(p, *i, *path, false)?,
        }
        .into(),
        DocSpec::SetKeyframe(i, path, t, v) => SetKeyframe {
            target: param_ref(p, *i, *path, true)?,
            keyframe: Keyframe::new(
                secs(*t),
                ParamValue::Number(*v as f64),
                Interpolation::Linear,
            ),
        }
        .into(),
        DocSpec::RemoveKeyframe(i, path, t) => RemoveKeyframe {
            target: param_ref(p, *i, *path, true)?,
            time: secs(i64::from(*t % 3)),
        }
        .into(),
        DocSpec::MoveKeyframe(i, path, from, to, v) => MoveKeyframe {
            target: param_ref(p, *i, *path, true)?,
            from: secs(i64::from(*from % 3)),
            keyframe: Keyframe::new(
                secs(*to),
                ParamValue::Number(*v as f64),
                Interpolation::Bezier,
            ),
        }
        .into(),
        DocSpec::SetEase(i, path, t, e) => SetEase {
            target: param_ref(p, *i, *path, true)?,
            time: secs(i64::from(*t % 3)),
            interpolation: [
                Interpolation::Hold,
                Interpolation::Linear,
                Interpolation::Bezier,
            ][usize::from(*e % 3)],
            ease: [Ease::None, Ease::In, Ease::Out, Ease::InOut][usize::from(*e % 4)],
            bezier: None,
        }
        .into(),
        _ => return None,
    })
}

fn precompose(comp: u8, mask: u8, new_comp: &str, new_layer: &str, p: &Project) -> Option<Op> {
    let c = pick(&p.compositions, comp)?;
    let layers = c
        .layers
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1 << (i % 8)) != 0)
        .map(|(_, l)| l.id.clone())
        .collect();
    Some(
        Precompose {
            composition: c.id.clone(),
            layers,
            new_composition: Id::new(new_comp),
            new_layer: Id::new(new_layer),
        }
        .into(),
    )
}

fn comp_op(spec: &DocSpec, p: &Project) -> Option<Op> {
    if let Some(op) = misc_op(spec, p) {
        return Some(op);
    }
    Some(match spec {
        DocSpec::Precompose {
            comp,
            mask,
            new_comp,
            new_layer,
        } => return precompose(*comp, *mask, new_comp, new_layer, p),
        DocSpec::Unprecompose(i) => {
            let nested: Vec<_> = p
                .compositions
                .iter()
                .flat_map(|c| c.layers.iter())
                .filter(|l| l.source.kind == mm_doc::NESTED_KIND)
                .map(|l| l.id.clone())
                .collect();
            Unprecompose {
                layer: pick(&nested, *i)?.clone(),
            }
            .into()
        }
        DocSpec::AddComposition(id, layers) => {
            let mut c = crate::common::comp(id, Vec::new(), Vec::new());
            c.layers = layers.iter().map(|l| build_layer(l, p, 0, 2)).collect();
            AddComposition { composition: c }.into()
        }
        DocSpec::RemoveComposition(i) => RemoveComposition {
            composition: pick(&p.compositions, *i)?.id.clone(),
        }
        .into(),
        _ => return None,
    })
}

/// レンダーターゲットとマーカーの操作。
fn misc_op(spec: &DocSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        DocSpec::AddRenderTarget(i, id) => AddRenderTarget {
            target: mm_doc::RenderTarget {
                id: Id::new(id),
                composition: pick(&p.compositions, *i)?.id.clone(),
                ..p.render_targets.first()?.clone()
            },
            // 奇数なら位置を指し、偶数なら末尾に足す（生成した値の並びを変えずに両方を通す）。
            index: (i % 2 == 1).then(|| usize::from(i / 2) % (p.render_targets.len() + 1)),
        }
        .into(),
        DocSpec::RemoveRenderTarget(i) => RemoveRenderTarget {
            target: pick(&p.render_targets, *i)?.id.clone(),
        }
        .into(),
        DocSpec::AddAsset(id) => AddAsset {
            asset: mm_doc::Asset {
                id: Id::new(id),
                path: format!("{id}.mp4"),
                content_hash: format!("hash-{id}"),
                color_space: mm_types::ColorSpace::bt709(),
                duration: secs(1),
                fps: None,
                timebase_offset: mm_types::RationalTime::ZERO,
            },
            index: None,
        }
        .into(),
        DocSpec::RemoveAsset(i) => RemoveAsset {
            asset: pick(&p.assets, *i)?.id.clone(),
        }
        .into(),
        DocSpec::AddMarker(i, id, t) => AddMarker {
            composition: pick(&p.compositions, *i)?.id.clone(),
            marker: marker(id, *t),
        }
        .into(),
        DocSpec::RemoveMarker(i) => {
            let markers: Vec<_> = p
                .compositions
                .iter()
                .flat_map(|c| c.markers.iter())
                .collect();
            RemoveMarker {
                marker: pick(&markers, *i)?.id.clone(),
            }
            .into()
        }
        _ => return None,
    })
}
