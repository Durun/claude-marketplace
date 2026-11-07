//! 段階 3 の操作（選別、話題、文字起こしの修正、レイヤーの端、隙間、カットの列）の種。

use super::build::{arb_cut, arb_id, build_cut, cut_id, layer_id, pick, CutSpec};
use crate::common::{quarters, secs};
use mm_doc::{Project, Rating, Selection, Topic};
use mm_ops::*;
use mm_types::Id;
use proptest::prelude::*;

/// 単語の ID の集まり。小さくして、同じ単語への操作が重なるようにする。
const WORDS: [&str; 4] = ["w0", "w1", "w2", "w3"];
const TAGS: [&str; 3] = ["a", "b", "c"];
const RATINGS: [Rating; 3] = [Rating::Unrated, Rating::Favorite, Rating::Rejected];

/// 段階 3 の操作の種。
#[derive(Debug, Clone)]
pub enum BinSpec {
    Mark(bool, u8, Option<i64>),
    AddSelection(String, u8, i64, i64),
    RemoveSelection(u8),
    Rate(u8, u8),
    Tag(u8, u8, bool),
    AddTopic(String, u8, i64, i64),
    RemoveTopic(u8),
    DeleteWords(u8, Vec<u8>, bool),
    CutWords(u8, u8, Vec<u8>, Vec<(i64, i64)>, bool),
    FixTranscript(u8, u8, Option<u8>),
    TrimLayer(u8, bool, i64),
    FitLayer(u8),
    Lift(u8),
    InsertCuts(u8, u8, Vec<CutSpec>),
    ExtractCuts(u8, u8, u8),
    InsertTopics(u8, Option<u8>, Vec<u8>, String),
}

pub fn arb_bin_spec() -> impl Strategy<Value = BinSpec> {
    let s = || any::<u8>();
    prop_oneof![
        (any::<bool>(), s(), prop::option::weighted(0.8, 0i64..12))
            .prop_map(|(o, a, t)| BinSpec::Mark(o, a, t)),
        (arb_id(), s(), 0i64..8, 1i64..6)
            .prop_map(|(i, a, t, l)| BinSpec::AddSelection(i, a, t, l)),
        s().prop_map(BinSpec::RemoveSelection),
        (s(), s()).prop_map(|(i, r)| BinSpec::Rate(i, r)),
        (s(), s(), any::<bool>()).prop_map(|(i, t, r)| BinSpec::Tag(i, t, r)),
        (arb_id(), s(), 0i64..8, 1i64..6).prop_map(|(i, a, t, l)| BinSpec::AddTopic(i, a, t, l)),
        s().prop_map(BinSpec::RemoveTopic),
        (s(), prop::collection::vec(s(), 1..3), any::<bool>())
            .prop_map(|(a, w, r)| BinSpec::DeleteWords(a, w, r)),
        (
            s(),
            s(),
            prop::collection::vec(s(), 0..3),
            prop::collection::vec((0i64..24, 1i64..8), 0..3),
            prop::bool::weighted(0.2),
        )
            .prop_map(|(a, q, w, r, b)| BinSpec::CutWords(a, q, w, r, b)),
        (s(), s(), prop::option::of(s())).prop_map(|(a, w, t)| BinSpec::FixTranscript(a, w, t)),
        (s(), any::<bool>(), -6i64..6).prop_map(|(l, o, b)| BinSpec::TrimLayer(l, o, b)),
        s().prop_map(BinSpec::FitLayer),
        s().prop_map(BinSpec::Lift),
        (s(), s(), prop::collection::vec(arb_cut(), 0..3))
            .prop_map(|(q, i, c)| BinSpec::InsertCuts(q, i, c)),
        (s(), s(), 0u8..3).prop_map(|(q, i, n)| BinSpec::ExtractCuts(q, i, n)),
        (
            s(),
            prop::option::of(s()),
            prop::collection::vec(s(), 0..3),
            arb_id()
        )
            .prop_map(|(q, i, t, p)| BinSpec::InsertTopics(q, i, t, p)),
    ]
}

fn word_ids(picks: &[u8]) -> Vec<mm_doc::WordId> {
    picks
        .iter()
        .filter_map(|w| pick(&WORDS, *w))
        .map(|w| Id::new(*w))
        .collect()
}

/// 選別と話題の ID（`rate` と `tag` が指す）。
fn items(p: &Project) -> Vec<String> {
    let sel = p.bins.selections.iter().map(|s| s.id.to_string());
    sel.chain(p.bins.topics.iter().map(|t| t.id.to_string()))
        .collect()
}

pub fn bin_op(spec: &BinSpec, p: &Project) -> Option<Op> {
    if let Some(op) = edit_op(spec, p) {
        return Some(op);
    }
    let asset = |i: u8| pick(&p.assets, i).map(|a| a.id.clone());
    Some(match spec {
        BinSpec::Mark(out, a, t) => {
            let (asset, at) = (asset(*a)?, t.map(secs));
            if *out {
                MarkOut { asset, at }.into()
            } else {
                MarkIn { asset, at }.into()
            }
        }
        BinSpec::AddSelection(id, a, t, l) => AddSelection {
            selection: Selection {
                id: Id::new(id),
                asset: asset(*a)?,
                in_point: secs(*t),
                out_point: secs(t + l),
                rating: Rating::Unrated,
                tags: Vec::new(),
            },
        }
        .into(),
        BinSpec::RemoveSelection(i) => RemoveSelection {
            selection: pick(&p.bins.selections, *i)?.id.clone(),
        }
        .into(),
        BinSpec::Rate(i, r) => Rate {
            target: pick(&items(p), *i)?.clone(),
            rating: *pick(&RATINGS, *r)?,
        }
        .into(),
        BinSpec::Tag(i, t, remove) => Tag {
            target: pick(&items(p), *i)?.clone(),
            tag: pick(&TAGS, *t)?.to_string(),
            remove: *remove,
        }
        .into(),
        BinSpec::AddTopic(id, a, t, l) => AddTopic {
            topic: Topic {
                id: Id::new(id),
                asset: asset(*a)?,
                in_point: secs(*t),
                out_point: secs(t + l),
                title: id.clone(),
                rating: Rating::Unrated,
                tags: Vec::new(),
            },
        }
        .into(),
        BinSpec::RemoveTopic(i) => RemoveTopic {
            topic: pick(&p.bins.topics, *i)?.id.clone(),
        }
        .into(),
        _ => return transcript_op(spec, p),
    })
}

/// 文字起こしの単語を消す・直す操作。
fn transcript_op(spec: &BinSpec, p: &Project) -> Option<Op> {
    let asset = |i: u8| pick(&p.assets, i).map(|a| a.id.clone());
    Some(match spec {
        BinSpec::DeleteWords(a, words, restore) => DeleteWords {
            asset: asset(*a)?,
            words: word_ids(words),
            restore: *restore,
        }
        .into(),
        BinSpec::CutWords(a, q, words, ranges, restore) => CutWords {
            sequence: pick(&p.sequences, *q)?.id.clone(),
            asset: asset(*a)?,
            words: word_ids(words),
            // 話題のカット（1 秒から 4 秒）に掛かるよう、4 分の 1 秒の刻みで 0 秒から 8 秒に置く。
            ranges: ranges
                .iter()
                .map(|(from, len)| SourceRange {
                    in_point: quarters(*from),
                    out_point: quarters(from + len),
                })
                .collect(),
            restore: *restore,
            splices: Vec::new(),
        }
        .into(),
        BinSpec::FixTranscript(a, w, t) => FixTranscript {
            asset: asset(*a)?,
            word: Id::new(*pick(&WORDS, *w)?),
            text: t.map(|t| format!("x{}", t % 3)),
        }
        .into(),
        _ => return None,
    })
}

/// レイヤーの端、隙間、カットの列の操作。
fn edit_op(spec: &BinSpec, p: &Project) -> Option<Op> {
    Some(match spec {
        BinSpec::TrimLayer(l, out, by) => TrimLayer {
            layer: layer_id(p, *l)?,
            edge: if *out { Edge::Out } else { Edge::In },
            by: quarters(*by),
        }
        .into(),
        BinSpec::FitLayer(l) => FitLayer {
            layer: {
                let layers: Vec<_> = p
                    .compositions
                    .iter()
                    .flat_map(|c| c.layers.iter())
                    .filter(|l| l.source.kind == mm_doc::SEQUENCE_KIND)
                    .map(|l| l.id.clone())
                    .collect();
                pick(&layers, *l)?.clone()
            },
            out_point: None,
            duration: None,
        }
        .into(),
        BinSpec::Lift(i) => Lift {
            cut: cut_id(p, *i)?,
        }
        .into(),
        BinSpec::InsertCuts(q, i, cuts) => {
            let s = pick(&p.sequences, *q)?;
            InsertCuts {
                sequence: s.id.clone(),
                index: usize::from(*i) % (s.cuts.len() + 1),
                cuts: cuts.iter().map(|c| build_cut(c, p)).collect(),
            }
            .into()
        }
        BinSpec::ExtractCuts(q, i, n) => {
            let s = pick(&p.sequences, *q)?;
            ExtractCuts {
                sequence: s.id.clone(),
                index: usize::from(*i) % (s.cuts.len() + 1),
                count: usize::from(*n),
            }
            .into()
        }
        BinSpec::InsertTopics(q, i, topics, prefix) => {
            let s = pick(&p.sequences, *q)?;
            let topics: Vec<_> = topics
                .iter()
                .filter_map(|t| pick(&p.bins.topics, *t).map(|t| t.id.clone()))
                .collect();
            InsertTopics {
                sequence: s.id.clone(),
                index: i.map(|i| usize::from(i) % (s.cuts.len() + 1)),
                cuts: (0..topics.len())
                    .map(|k| Id::new(format!("{prefix}{k}")))
                    .collect(),
                topics,
            }
            .into()
        }
        _ => return None,
    })
}
