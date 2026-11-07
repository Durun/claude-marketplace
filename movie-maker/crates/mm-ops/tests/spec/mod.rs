//! 操作の種と、プロジェクトのデータの ID への当てはめ。性質テストが使う。

mod bins;
mod build;
mod params;
mod shorts;

use crate::common::{marker, quarters, secs};
use build::{build_cut, build_layer, cut_id, layer_id, pick};
use mm_doc::{Ease, Interpolation, Keyframe, OriginRelation, Project, Sequence};
use mm_ops::*;
use mm_types::{Id, ParamValue};
use params::{param_ref, value_for};
use proptest::prelude::*;

pub use build::{arb_cut, arb_id, arb_layer, CutSpec, LayerSpec};

/// 操作の種。プロジェクトのデータを見ずに作り、[`materialize`] でプロジェクトのデータの ID に当てはめる。
#[derive(Debug, Clone)]
pub enum Spec {
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
    ParentLayer(u8, u8),
    AttachLayer(u8, u8),
    LockLayer(u8, bool),
    /// レイヤーの番号と、音の設定の種（0 は外す、奇数はミュート、4 以上はソロ）。
    LayerAudio(u8, u8),
    /// コンポジションの番号と、ミキサーの設定の種（0 は外す、偶数はダッキングあり）。
    Mixer(u8, u8),
    Precompose {
        comp: u8,
        mask: u8,
        new_comp: String,
        new_layer: String,
    },
    Unprecompose(u8),
    AddComposition(String, Vec<LayerSpec>),
    RemoveComposition(u8),
    AddSequence(String, Vec<CutSpec>, Option<u8>),
    /// 元の番号、複製か、新しい ID、カットの ID の接頭辞、範囲（元のカットの番号の組）。
    ForkSequence(u8, bool, String, String, Option<(u8, u8)>),
    RemoveSequence(u8),
    AddRenderTarget(u8, String),
    RemoveRenderTarget(u8),
    AddAsset(String),
    RemoveAsset(u8),
    AddMarker(u8, String, i64),
    RemoveMarker(u8),
    Insert(u8, u8, CutSpec),
    Overwrite(u8, CutSpec),
    Extract(u8),
    Split(u8, i64, String),
    Join(u8),
    Ripple(u8, bool, i64),
    Roll(u8, i64),
    Slip(u8, i64),
    Slide(u8, i64),
    Disable(u8, bool),
    /// 段階 3 の操作。
    Bin(bins::BinSpec),
    /// 本編とショートの操作。
    Short(shorts::ShortSpec),
}

/// プロジェクトのデータを編集する操作の種（レイヤー、パラメータ、コンポジション）。
fn arb_doc_spec() -> impl Strategy<Value = Spec> {
    let s = || any::<u8>();
    let t = || -6i64..6;
    prop_oneof![
        (s(), s(), 0i64..5, 1i64..5, arb_layer()).prop_map(|(comp, at, start, len, layer)| {
            Spec::AddLayer {
                comp,
                at,
                start,
                len,
                layer,
            }
        }),
        s().prop_map(Spec::RemoveLayer),
        (s(), s()).prop_map(|(a, b)| Spec::MoveLayer(a, b)),
        (s(), s(), t()).prop_map(|(a, b, c)| Spec::SetParam(a, b, c)),
        (s(), s()).prop_map(|(a, b)| Spec::RemoveParam(a, b)),
        (s(), s(), 0i64..4, t()).prop_map(|(a, b, c, d)| Spec::SetKeyframe(a, b, c, d)),
        (s(), s(), s()).prop_map(|(a, b, c)| Spec::RemoveKeyframe(a, b, c)),
        (s(), s(), s(), s()).prop_map(|(a, b, c, d)| Spec::SetEase(a, b, c, d)),
        (s(), s()).prop_map(|(a, b)| Spec::ParentLayer(a, b)),
        (s(), s()).prop_map(|(a, b)| Spec::AttachLayer(a, b)),
        (s(), any::<bool>()).prop_map(|(a, b)| Spec::LockLayer(a, b)),
        (s(), s()).prop_map(|(a, b)| Spec::LayerAudio(a, b)),
        (s(), s()).prop_map(|(a, b)| Spec::Mixer(a, b)),
        (s(), 1u8..16, arb_id(), arb_id()).prop_map(|(comp, mask, new_comp, new_layer)| {
            Spec::Precompose {
                comp,
                mask,
                new_comp,
                new_layer,
            }
        }),
        s().prop_map(Spec::Unprecompose),
        (arb_id(), prop::collection::vec(arb_layer(), 0..3))
            .prop_map(|(id, layers)| Spec::AddComposition(id, layers)),
        s().prop_map(Spec::RemoveComposition),
        (s(), arb_id()).prop_map(|(a, b)| Spec::AddRenderTarget(a, b)),
        s().prop_map(Spec::RemoveRenderTarget),
        arb_id().prop_map(Spec::AddAsset),
        s().prop_map(Spec::RemoveAsset),
        (s(), arb_id(), 0i64..10).prop_map(|(a, b, c)| Spec::AddMarker(a, b, c)),
        s().prop_map(Spec::RemoveMarker),
    ]
}

/// sequence とカットを編集する操作の種。
fn arb_seq_spec() -> impl Strategy<Value = Spec> {
    let s = || any::<u8>();
    let t = || -6i64..6;
    prop_oneof![
        (
            arb_id(),
            prop::collection::vec(arb_cut(), 0..3),
            prop::option::weighted(0.3, s())
        )
            .prop_map(|(id, cuts, origin)| Spec::AddSequence(id, cuts, origin)),
        (
            s(),
            any::<bool>(),
            arb_id(),
            arb_id(),
            prop::option::of((s(), s()))
        )
            .prop_map(|(a, b, c, d, e)| Spec::ForkSequence(a, b, c, d, e)),
        s().prop_map(Spec::RemoveSequence),
        (s(), s(), arb_cut()).prop_map(|(a, b, c)| Spec::Insert(a, b, c)),
        (s(), arb_cut()).prop_map(|(a, b)| Spec::Overwrite(a, b)),
        s().prop_map(Spec::Extract),
        (s(), 1i64..12, arb_id()).prop_map(|(a, b, c)| Spec::Split(a, b, c)),
        s().prop_map(Spec::Join),
        (s(), any::<bool>(), t()).prop_map(|(a, b, c)| Spec::Ripple(a, b, c)),
        (s(), t()).prop_map(|(a, b)| Spec::Roll(a, b)),
        (s(), t()).prop_map(|(a, b)| Spec::Slip(a, b)),
        (s(), t()).prop_map(|(a, b)| Spec::Slide(a, b)),
        (s(), any::<bool>()).prop_map(|(a, b)| Spec::Disable(a, b)),
    ]
}

pub fn arb_spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        3 => arb_doc_spec(),
        2 => arb_seq_spec(),
        3 => bins::arb_bin_spec().prop_map(Spec::Bin),
        1 => shorts::arb_short_spec().prop_map(Spec::Short),
    ]
}

/// 種を今のプロジェクトのデータの ID に当てはめる。当てはめる相手が無ければ `None`。
pub fn materialize(spec: &Spec, p: &Project) -> Option<Op> {
    if let Spec::Bin(b) = spec {
        return bins::bin_op(b, p);
    }
    if let Spec::Short(s) = spec {
        return shorts::short_op(s, p);
    }
    layer_op(spec, p)
        .or_else(|| param_op(spec, p))
        .or_else(|| comp_op(spec, p))
        .or_else(|| cut_op(spec, p))
}

fn layer_op(spec: &Spec, p: &Project) -> Option<Op> {
    Some(match spec {
        Spec::AddLayer {
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
        Spec::RemoveLayer(i) => RemoveLayer {
            layer: layer_id(p, *i)?,
        }
        .into(),
        Spec::MoveLayer(i, j) => MoveLayer {
            layer: layer_id(p, *i)?,
            index: usize::from(*j) % 3,
        }
        .into(),
        Spec::ParentLayer(i, j) => ParentLayer {
            layer: layer_id(p, *i)?,
            parent: if *j % 4 == 0 { None } else { layer_id(p, *j) },
        }
        .into(),
        Spec::AttachLayer(i, j) => AttachLayer {
            layer: layer_id(p, *i)?,
            cut: if *j % 4 == 0 { None } else { cut_id(p, *j) },
        }
        .into(),
        Spec::LockLayer(i, lock) => LockLayer {
            layer: layer_id(p, *i)?,
            lock: *lock,
        }
        .into(),
        Spec::LayerAudio(i, seed) => SetLayerAudio {
            layer: layer_id(p, *i)?,
            audio: build::layer_audio(*seed),
        }
        .into(),
        _ => return None,
    })
}

fn param_op(spec: &Spec, p: &Project) -> Option<Op> {
    Some(match spec {
        Spec::SetParam(i, path, v) => {
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
        Spec::RemoveParam(i, path) => RemoveParam {
            target: param_ref(p, *i, *path, false)?,
        }
        .into(),
        Spec::SetKeyframe(i, path, t, v) => SetKeyframe {
            target: param_ref(p, *i, *path, true)?,
            keyframe: Keyframe::new(
                secs(*t),
                ParamValue::Number(*v as f64),
                Interpolation::Linear,
            ),
        }
        .into(),
        Spec::RemoveKeyframe(i, path, t) => RemoveKeyframe {
            target: param_ref(p, *i, *path, true)?,
            time: secs(i64::from(*t % 3)),
        }
        .into(),
        Spec::SetEase(i, path, t, e) => SetEase {
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

fn comp_op(spec: &Spec, p: &Project) -> Option<Op> {
    if let Some(op) = misc_op(spec, p) {
        return Some(op);
    }
    Some(match spec {
        Spec::Precompose {
            comp,
            mask,
            new_comp,
            new_layer,
        } => return precompose(*comp, *mask, new_comp, new_layer, p),
        Spec::Unprecompose(i) => {
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
        Spec::AddComposition(id, layers) => {
            let mut c = crate::common::comp(id, Vec::new(), Vec::new());
            c.layers = layers.iter().map(|l| build_layer(l, p, 0, 2)).collect();
            AddComposition { composition: c }.into()
        }
        Spec::RemoveComposition(i) => RemoveComposition {
            composition: pick(&p.compositions, *i)?.id.clone(),
        }
        .into(),
        Spec::Mixer(i, seed) => SetMixer {
            composition: pick(&p.compositions, *i)?.id.clone(),
            mixer: build::comp_mixer(*seed),
        }
        .into(),
        _ => return None,
    })
}

fn fork_sequence(
    p: &Project,
    (i, fork): (u8, bool),
    new_seq: &str,
    prefix: &str,
    range: Option<(u8, u8)>,
) -> Option<Op> {
    let s = pick(&p.sequences, i)?;
    let relation = if fork {
        OriginRelation::Fork
    } else {
        OriginRelation::Reference
    };
    let (range, count) = match range {
        Some((a, b)) if !s.cuts.is_empty() => {
            let n = s.cuts.len();
            let (a, b) = (usize::from(a) % n, usize::from(b) % n);
            let (first, last) = (a.min(b), a.max(b));
            let range = CutRange {
                first: s.cuts[first].id.clone(),
                last: s.cuts[last].id.clone(),
            };
            (Some(range), last - first + 1)
        }
        _ => (None, s.cuts.len()),
    };
    Some(
        ForkSequence {
            sequence: s.id.clone(),
            new_sequence: Id::new(new_seq),
            relation,
            cuts: (0..count)
                .map(|k| Id::new(format!("{prefix}{k}")))
                .collect(),
            range,
        }
        .into(),
    )
}

fn sequence_op(spec: &Spec, p: &Project) -> Option<Op> {
    Some(match spec {
        Spec::AddSequence(id, cuts, origin) => AddSequence {
            sequence: Sequence {
                id: Id::new(id),
                cuts: cuts.iter().map(|c| build_cut(c, p)).collect(),
                origin: origin.and_then(|i| pick(&p.sequences, i)).map(|s| {
                    mm_doc::SequenceOrigin {
                        sequence: s.id.clone(),
                        relation: OriginRelation::Reference,
                    }
                }),
            },
        }
        .into(),
        Spec::ForkSequence(i, fork, new_seq, prefix, range) => {
            return fork_sequence(p, (*i, *fork), new_seq, prefix, *range)
        }
        Spec::RemoveSequence(i) => RemoveSequence {
            sequence: pick(&p.sequences, *i)?.id.clone(),
        }
        .into(),
        Spec::Insert(si, i, cut) => {
            let s = pick(&p.sequences, *si)?;
            Insert {
                sequence: s.id.clone(),
                index: usize::from(*i) % (s.cuts.len() + 1),
                cut: build_cut(cut, p),
            }
            .into()
        }
        Spec::Overwrite(i, cut) => Overwrite {
            cut: cut_id(p, *i)?,
            replacement: build_cut(cut, p),
        }
        .into(),
        _ => return None,
    })
}

/// 時間を動かすカットの操作（ripple、roll、slip、slide、disable）。
fn timing_op(spec: &Spec, p: &Project) -> Option<Op> {
    Some(match spec {
        Spec::Ripple(i, out, by) => Ripple {
            cut: cut_id(p, *i)?,
            edge: if *out { Edge::Out } else { Edge::In },
            by: quarters(*by),
        }
        .into(),
        Spec::Roll(i, by) => Roll {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        Spec::Slip(i, by) => Slip {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        Spec::Slide(i, by) => Slide {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        Spec::Disable(i, d) => Disable {
            cut: cut_id(p, *i)?,
            disabled: *d,
        }
        .into(),
        _ => return None,
    })
}

/// レンダーターゲットとマーカーの操作。
fn misc_op(spec: &Spec, p: &Project) -> Option<Op> {
    Some(match spec {
        Spec::AddRenderTarget(i, id) => AddRenderTarget {
            target: mm_doc::RenderTarget {
                id: Id::new(id),
                composition: pick(&p.compositions, *i)?.id.clone(),
                ..p.render_targets.first()?.clone()
            },
            // 奇数なら位置を指し、偶数なら末尾に足す（生成した値の並びを変えずに両方を通す）。
            index: (i % 2 == 1).then(|| usize::from(i / 2) % (p.render_targets.len() + 1)),
        }
        .into(),
        Spec::RemoveRenderTarget(i) => RemoveRenderTarget {
            target: pick(&p.render_targets, *i)?.id.clone(),
        }
        .into(),
        Spec::AddAsset(id) => AddAsset {
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
        Spec::RemoveAsset(i) => RemoveAsset {
            asset: pick(&p.assets, *i)?.id.clone(),
        }
        .into(),
        Spec::AddMarker(i, id, t) => AddMarker {
            composition: pick(&p.compositions, *i)?.id.clone(),
            marker: marker(id, *t),
        }
        .into(),
        Spec::RemoveMarker(i) => {
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

fn cut_op(spec: &Spec, p: &Project) -> Option<Op> {
    if let Some(op) = sequence_op(spec, p).or_else(|| timing_op(spec, p)) {
        return Some(op);
    }
    Some(match spec {
        Spec::Extract(i) => Extract {
            cut: cut_id(p, *i)?,
        }
        .into(),
        Spec::Split(i, at, id) => Split {
            cut: cut_id(p, *i)?,
            at: quarters(*at),
            new_cut: Id::new(id),
        }
        .into(),
        Spec::Join(i) => {
            let s = pick(&p.sequences, *i)?;
            let first = pick(&s.cuts, *i)?;
            let index = s.cuts.iter().position(|c| c.id == first.id)?;
            Join {
                cut: first.id.clone(),
                next: s.cuts.get(index + 1)?.id.clone(),
            }
            .into()
        }
        _ => return None,
    })
}
