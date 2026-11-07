//! sequence とカットを編集する操作の種。

use super::build::{arb_cut, arb_id, build_cut, cut_id, pick, CutSpec};
use crate::common::quarters;
use mm_doc::{OriginRelation, Project, Sequence};
use mm_ops::*;
use mm_types::Id;
use proptest::prelude::*;

/// sequence とカットを編集する操作の種。
#[derive(Debug, Clone)]
pub enum SeqSpec {
    AddSequence(String, Vec<CutSpec>, Option<u8>),
    /// 元の番号、複製か、新しい ID、カットの ID の接頭辞、範囲（元のカットの番号の組）。
    ForkSequence(u8, bool, String, String, Option<(u8, u8)>),
    RemoveSequence(u8),
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
    /// カット、トランジションの長さ（1/4 秒。`None` で外す、負は弾かれる）。
    SetTransition(u8, Option<i64>),
}

pub fn arb() -> impl Strategy<Value = SeqSpec> {
    let s = || any::<u8>();
    let t = || -6i64..6;
    prop_oneof![
        (
            arb_id(),
            prop::collection::vec(arb_cut(), 0..3),
            prop::option::weighted(0.3, s())
        )
            .prop_map(|(id, cuts, origin)| SeqSpec::AddSequence(id, cuts, origin)),
        (
            s(),
            any::<bool>(),
            arb_id(),
            arb_id(),
            prop::option::of((s(), s()))
        )
            .prop_map(|(a, b, c, d, e)| SeqSpec::ForkSequence(a, b, c, d, e)),
        s().prop_map(SeqSpec::RemoveSequence),
        (s(), s(), arb_cut()).prop_map(|(a, b, c)| SeqSpec::Insert(a, b, c)),
        (s(), arb_cut()).prop_map(|(a, b)| SeqSpec::Overwrite(a, b)),
        s().prop_map(SeqSpec::Extract),
        (s(), 1i64..12, arb_id()).prop_map(|(a, b, c)| SeqSpec::Split(a, b, c)),
        s().prop_map(SeqSpec::Join),
        (s(), any::<bool>(), t()).prop_map(|(a, b, c)| SeqSpec::Ripple(a, b, c)),
        (s(), t()).prop_map(|(a, b)| SeqSpec::Roll(a, b)),
        (s(), t()).prop_map(|(a, b)| SeqSpec::Slip(a, b)),
        (s(), t()).prop_map(|(a, b)| SeqSpec::Slide(a, b)),
        (s(), any::<bool>()).prop_map(|(a, b)| SeqSpec::Disable(a, b)),
        (s(), prop::option::of(-1i64..8)).prop_map(|(a, b)| SeqSpec::SetTransition(a, b)),
    ]
}

/// 種を今のプロジェクトのデータの ID に当てはめる。当てはめる相手が無ければ `None`。
pub fn op(spec: &SeqSpec, p: &Project) -> Option<Op> {
    cut_op(spec, p)
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

fn sequence_op(spec: &SeqSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        SeqSpec::AddSequence(id, cuts, origin) => AddSequence {
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
        SeqSpec::ForkSequence(i, fork, new_seq, prefix, range) => {
            return fork_sequence(p, (*i, *fork), new_seq, prefix, *range)
        }
        SeqSpec::RemoveSequence(i) => RemoveSequence {
            sequence: pick(&p.sequences, *i)?.id.clone(),
        }
        .into(),
        SeqSpec::Insert(si, i, cut) => {
            let s = pick(&p.sequences, *si)?;
            Insert {
                sequence: s.id.clone(),
                index: usize::from(*i) % (s.cuts.len() + 1),
                cut: build_cut(cut, p),
            }
            .into()
        }
        SeqSpec::Overwrite(i, cut) => Overwrite {
            cut: cut_id(p, *i)?,
            replacement: build_cut(cut, p),
        }
        .into(),
        _ => return None,
    })
}

/// 時間を動かすカットの操作（ripple、roll、slip、slide、disable）。
fn timing_op(spec: &SeqSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        SeqSpec::Ripple(i, out, by) => Ripple {
            cut: cut_id(p, *i)?,
            edge: if *out { Edge::Out } else { Edge::In },
            by: quarters(*by),
        }
        .into(),
        SeqSpec::Roll(i, by) => Roll {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        SeqSpec::Slip(i, by) => Slip {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        SeqSpec::Slide(i, by) => Slide {
            cut: cut_id(p, *i)?,
            by: quarters(*by),
        }
        .into(),
        SeqSpec::Disable(i, d) => Disable {
            cut: cut_id(p, *i)?,
            disabled: *d,
        }
        .into(),
        SeqSpec::SetTransition(i, d) => SetTransition {
            cut: cut_id(p, *i)?,
            transition: d.map(|q| mm_doc::Transition {
                node: mm_doc::Node::new("transition.dissolve", 1),
                duration: quarters(q),
            }),
        }
        .into(),
        _ => return None,
    })
}

fn cut_op(spec: &SeqSpec, p: &Project) -> Option<Op> {
    if let Some(op) = sequence_op(spec, p).or_else(|| timing_op(spec, p)) {
        return Some(op);
    }
    Some(match spec {
        SeqSpec::Extract(i) => Extract {
            cut: cut_id(p, *i)?,
        }
        .into(),
        SeqSpec::Split(i, at, id) => Split {
            cut: cut_id(p, *i)?,
            at: quarters(*at),
            new_cut: Id::new(id),
        }
        .into(),
        SeqSpec::Join(i) => {
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
