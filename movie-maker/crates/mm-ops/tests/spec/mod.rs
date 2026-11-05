//! 命令の種と、文書の ID への当てはめ。性質テストが使う。

use super::common::{cut, layer, marker, quarters, secs};
use mm_doc::{
    Composition, CutId, Ease, Interpolation, Keyframe, OriginRelation, Project, Sequence,
};
use mm_ops::*;
use mm_types::{Id, LayerId, ParamValue};
use proptest::prelude::*;

/// 命令の種。文書を見ずに作り、[`materialize`] で文書の ID に当てはめる。
#[derive(Debug, Clone)]
pub enum Spec {
    AddLayer {
        comp: u8,
        at: u8,
        start: i64,
        len: i64,
        refs: u8,
    },
    RemoveLayer(u8),
    MoveLayer(u8, u8),
    SetParam(u8, u8, i64),
    RemoveParam(u8),
    SetKeyframe(u8, i64, i64),
    RemoveKeyframe(u8, u8),
    SetEase(u8, u8, u8),
    ParentLayer(u8, u8),
    AttachLayer(u8, u8),
    LockLayer(u8, bool),
    Precompose(u8),
    AddComposition(u8),
    RemoveComposition(u8),
    AddSequence(u8),
    ForkSequence(bool),
    RemoveSequence,
    AddRenderTarget(u8),
    RemoveRenderTarget,
    AddMarker(i64),
    RemoveMarker(u8),
    Insert(u8, i64, i64),
    Overwrite(u8, i64, i64),
    Extract(u8),
    Split(u8, i64),
    Join(u8),
    Ripple(u8, bool, i64),
    Roll(u8, i64),
    Slip(u8, i64),
    Slide(u8, i64),
    Disable(u8, bool),
}

pub fn arb_spec() -> impl Strategy<Value = Spec> {
    let small = 0u8..8;
    let t = -6i64..6;
    prop_oneof![
        (
            small.clone(),
            small.clone(),
            0i64..5,
            1i64..5,
            small.clone()
        )
            .prop_map(|(comp, at, start, len, refs)| Spec::AddLayer {
                comp,
                at,
                start,
                len,
                refs
            }),
        small.clone().prop_map(Spec::RemoveLayer),
        (small.clone(), small.clone()).prop_map(|(a, b)| Spec::MoveLayer(a, b)),
        (small.clone(), small.clone(), t.clone()).prop_map(|(a, b, c)| Spec::SetParam(a, b, c)),
        small.clone().prop_map(Spec::RemoveParam),
        (small.clone(), 0i64..4, t.clone()).prop_map(|(a, b, c)| Spec::SetKeyframe(a, b, c)),
        (small.clone(), small.clone()).prop_map(|(a, b)| Spec::RemoveKeyframe(a, b)),
        (small.clone(), small.clone(), small.clone()).prop_map(|(a, b, c)| Spec::SetEase(a, b, c)),
        (small.clone(), small.clone()).prop_map(|(a, b)| Spec::ParentLayer(a, b)),
        (small.clone(), small.clone()).prop_map(|(a, b)| Spec::AttachLayer(a, b)),
        (small.clone(), any::<bool>()).prop_map(|(a, b)| Spec::LockLayer(a, b)),
        (1u8..8).prop_map(Spec::Precompose),
        small.clone().prop_map(Spec::AddComposition),
        small.clone().prop_map(Spec::RemoveComposition),
        small.clone().prop_map(Spec::AddSequence),
        any::<bool>().prop_map(Spec::ForkSequence),
        Just(Spec::RemoveSequence),
        small.clone().prop_map(Spec::AddRenderTarget),
        Just(Spec::RemoveRenderTarget),
        (0i64..10).prop_map(Spec::AddMarker),
        small.clone().prop_map(Spec::RemoveMarker),
        (small.clone(), 0i64..4, 1i64..4).prop_map(|(a, b, c)| Spec::Insert(a, b, c)),
        (small.clone(), 0i64..4, 1i64..4).prop_map(|(a, b, c)| Spec::Overwrite(a, b, c)),
        small.clone().prop_map(Spec::Extract),
        (small.clone(), 1i64..12).prop_map(|(a, b)| Spec::Split(a, b)),
        small.clone().prop_map(Spec::Join),
        (small.clone(), any::<bool>(), t.clone()).prop_map(|(a, b, c)| Spec::Ripple(a, b, c)),
        (small.clone(), t.clone()).prop_map(|(a, b)| Spec::Roll(a, b)),
        (small.clone(), t.clone()).prop_map(|(a, b)| Spec::Slip(a, b)),
        (small.clone(), t.clone()).prop_map(|(a, b)| Spec::Slide(a, b)),
        (small, any::<bool>()).prop_map(|(a, b)| Spec::Disable(a, b)),
    ]
}

fn pick<T>(items: &[T], i: u8) -> Option<&T> {
    if items.is_empty() {
        None
    } else {
        items.get(usize::from(i) % items.len())
    }
}

fn layer_id(p: &Project, i: u8) -> Option<LayerId> {
    let layers: Vec<_> = p
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .map(|l| l.id.clone())
        .collect();
    pick(&layers, i).cloned()
}

fn cut_id(p: &Project, i: u8) -> Option<CutId> {
    let cuts: Vec<_> = p
        .sequences
        .iter()
        .flat_map(|s| s.cuts.iter())
        .map(|c| c.id.clone())
        .collect();
    pick(&cuts, i).cloned()
}

const PARAM_PATHS: [&str; 5] = [
    "opacity",
    "transform.position",
    "transform.anchor",
    "source.color",
    "source.extra",
];

fn param_ref(p: &Project, i: u8, path: u8) -> Option<ParamRef> {
    let path = PARAM_PATHS[usize::from(path) % PARAM_PATHS.len()];
    if path.starts_with("source") && i % 2 == 1 {
        return Some(ParamRef::cut(&cut_id(p, i)?, path));
    }
    Some(ParamRef::layer(&layer_id(p, i)?, path))
}

fn value_for(path: &str, v: i64) -> ParamValue {
    if path.contains("position") || path.contains("anchor") {
        ParamValue::Vec2([v as f64, 0.0])
    } else {
        ParamValue::Number(v as f64)
    }
}

/// 種を今の文書の ID に当てはめる。当てはめる相手が無ければ `None`。
pub fn materialize(spec: &Spec, p: &Project) -> Option<Op> {
    use Spec as S;
    let op: Op = match spec {
        S::AddLayer {
            comp,
            at,
            start,
            len,
            refs,
        } => {
            let c = pick(&p.compositions, *comp)?;
            let mut l = layer("lx", *start, 0, *len);
            if refs & 1 == 1 {
                l.parent = layer_id(p, *at);
            }
            if refs & 2 == 2 {
                l.attach_to = cut_id(p, *at);
            }
            AddLayer {
                composition: c.id.clone(),
                layer: l,
                index: Some(usize::from(*at) % (c.layers.len() + 1)),
            }
            .into()
        }
        S::RemoveLayer(i) => RemoveLayer {
            layer: layer_id(p, *i)?,
        }
        .into(),
        S::MoveLayer(i, j) => MoveLayer {
            layer: layer_id(p, *i)?,
            index: usize::from(*j) % 3,
        }
        .into(),
        S::SetParam(i, path, v) => {
            let target = param_ref(p, *i, *path)?;
            let value = value_for(&target.path, *v);
            SetParam {
                target,
                value,
                keyframes: (*v % 3 == 0).then(Vec::new),
            }
            .into()
        }
        S::RemoveParam(i) => RemoveParam {
            target: param_ref(p, *i, 3 + (*i % 2))?,
        }
        .into(),
        S::SetKeyframe(i, t, v) => SetKeyframe {
            target: param_ref(p, *i, 0)?,
            keyframe: Keyframe::new(
                secs(*t),
                ParamValue::Number(*v as f64),
                Interpolation::Linear,
            ),
        }
        .into(),
        S::RemoveKeyframe(i, t) => RemoveKeyframe {
            target: param_ref(p, *i, 0)?,
            time: secs(i64::from(*t % 3)),
        }
        .into(),
        S::SetEase(i, t, e) => SetEase {
            target: param_ref(p, *i, 0)?,
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
        S::ParentLayer(i, j) => ParentLayer {
            layer: layer_id(p, *i)?,
            parent: if *j == 0 { None } else { layer_id(p, *j) },
        }
        .into(),
        S::AttachLayer(i, j) => AttachLayer {
            layer: layer_id(p, *i)?,
            cut: if *j == 0 { None } else { cut_id(p, *j) },
        }
        .into(),
        S::LockLayer(i, lock) => LockLayer {
            layer: layer_id(p, *i)?,
            lock: *lock,
        }
        .into(),
        S::Precompose(mask) => {
            let c = p.compositions.first()?;
            let layers: Vec<_> = c
                .layers
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, l)| l.id.clone())
                .collect();
            Precompose {
                composition: c.id.clone(),
                layers,
                new_composition: Id::new("cx"),
                new_layer: Id::new("lx"),
            }
            .into()
        }
        S::AddComposition(i) => {
            let mut layers = Vec::new();
            if i % 2 == 1 {
                layers.push(layer("ly", 0, 0, 2));
            }
            AddComposition {
                composition: Composition {
                    id: Id::new("cx"),
                    ..super::common::comp("cx", layers, Vec::new())
                },
            }
            .into()
        }
        S::RemoveComposition(i) => RemoveComposition {
            composition: pick(&p.compositions, *i)?.id.clone(),
        }
        .into(),
        S::AddSequence(i) => AddSequence {
            sequence: Sequence {
                id: Id::new("sx"),
                cuts: (0..(*i % 3))
                    .map(|k| cut(&format!("kx{k}"), 0, 2, (1, 1)))
                    .collect(),
                origin: None,
            },
        }
        .into(),
        S::ForkSequence(fork) => {
            let s = p.sequences.first()?;
            ForkSequence {
                sequence: s.id.clone(),
                new_sequence: Id::new("sx"),
                relation: if *fork {
                    OriginRelation::Fork
                } else {
                    OriginRelation::Reference
                },
                cuts: (0..s.cuts.len())
                    .map(|k| Id::new(format!("kx{k}")))
                    .collect(),
            }
            .into()
        }
        S::RemoveSequence => RemoveSequence {
            sequence: p.sequences.first()?.id.clone(),
        }
        .into(),
        S::AddRenderTarget(i) => AddRenderTarget {
            target: mm_doc::RenderTarget {
                id: Id::new("rx"),
                composition: pick(&p.compositions, *i)?.id.clone(),
                ..p.render_targets.first()?.clone()
            },
        }
        .into(),
        S::RemoveRenderTarget => RemoveRenderTarget {
            target: p.render_targets.first()?.id.clone(),
        }
        .into(),
        S::AddMarker(t) => AddMarker {
            composition: p.compositions.first()?.id.clone(),
            marker: marker("mx", *t),
        }
        .into(),
        S::RemoveMarker(i) => RemoveMarker {
            marker: pick(&p.compositions.first()?.markers, *i)?.id.clone(),
        }
        .into(),
        S::Insert(i, in_s, len) => {
            let s = p.sequences.first()?;
            Insert {
                sequence: s.id.clone(),
                index: usize::from(*i) % (s.cuts.len() + 1),
                cut: cut("kx", *in_s, *in_s + *len, (1, 1)),
            }
            .into()
        }
        S::Overwrite(i, in_s, len) => Overwrite {
            cut: cut_id(p, *i)?,
            replacement: cut("kx", *in_s, *in_s + *len, (1, 1)),
        }
        .into(),
        S::Extract(i) => Extract {
            cut: cut_id(p, *i)?,
        }
        .into(),
        S::Split(i, at) => Split {
            cut: cut_id(p, *i)?,
            at: quarters(*at),
            new_cut: Id::new("kx"),
        }
        .into(),
        S::Join(i) => {
            let s = p.sequences.first()?;
            let first = pick(&s.cuts, *i)?;
            let index = s.cuts.iter().position(|c| c.id == first.id)?;
            Join {
                cut: first.id.clone(),
                next: s.cuts.get(index + 1)?.id.clone(),
            }
            .into()
        }
        S::Ripple(i, out, by) => Ripple {
            cut: cut_id(p, *i)?,
            edge: if *out { Edge::Out } else { Edge::In },
            by: quarters(*by),
        }
        .into(),
        S::Roll(i, by) => Roll {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        S::Slip(i, by) => Slip {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        S::Slide(i, by) => Slide {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        S::Disable(i, d) => Disable {
            cut: cut_id(p, *i)?,
            disabled: *d,
        }
        .into(),
    };
    Some(op)
}
