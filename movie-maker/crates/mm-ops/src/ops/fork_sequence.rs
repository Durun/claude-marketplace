//! `forkSequence`。sequence の全部か続いたカットの範囲を、参照か複製で分岐する。

use serde::{Deserialize, Serialize};

use mm_doc::{
    Cut, CutBase, CutId, CutOverrides, OriginRelation, Project, Sequence, SequenceId,
    SequenceOrigin,
};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_refs, invalid, RemoveSequence};
use crate::touches::Touches;

/// `sequence` のカットをコピーして `new_sequence` を作る。カットは `cuts` の ID でコピーし（数が合うこと）、
/// 各カットの `origin` に元のカットを、`origin_base` に元の値を残す（元が変わったら知らせる比べ先）。
///
/// - 参照（`reference`）: 元のカットを直すとショートのカットも変わる。in/out は `overrideCut` で上書きできる。
/// - 複製（`fork`）: 分岐する。元が変わっても従わない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkSequence {
    pub sequence: SequenceId,
    pub new_sequence: SequenceId,
    pub relation: OriginRelation,
    /// 新しいカットの ID。コピーする元のカットと同じ数。
    pub cuts: Vec<CutId>,
    /// コピーする続いたカットの範囲（両端を含む）。省くと全部。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<CutRange>,
}

/// sequence の中の続いたカットの範囲。`first` が `last` より前か同じ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutRange {
    pub first: CutId,
    pub last: CutId,
}

impl ForkSequence {
    /// コピーする元のカット。
    fn source_cuts<'a>(&self, project: &'a Project) -> Result<&'a [Cut]> {
        let cuts = &project.sequence(&self.sequence)?.cuts;
        let Some(range) = &self.range else {
            return Ok(cuts);
        };
        let at = |id: &CutId| {
            cuts.iter()
                .position(|c| &c.id == id)
                .ok_or_else(|| invalid(format!("カット {id} は sequence {} に無い", self.sequence)))
        };
        let (first, last) = (at(&range.first)?, at(&range.last)?);
        if first > last {
            return Err(invalid(format!(
                "範囲の {} が {} より後ろ",
                range.first, range.last
            )));
        }
        Ok(&cuts[first..=last])
    }

    fn copy(&self, cut: &Cut, id: &CutId) -> Cut {
        let mut c = cut.clone();
        c.id = id.clone();
        c.origin = Some(cut.id.clone());
        c.overrides = CutOverrides::default();
        c.origin_base = Some(CutBase::of(cut));
        c
    }
}

impl Apply for ForkSequence {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.sequence);
        t.add(&self.new_sequence);
        for c in self.source_cuts(project)? {
            t.add(&c.id);
            cut_refs(c, &mut t);
        }
        for c in &self.cuts {
            t.add(c);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.sequence(&self.new_sequence).is_ok() {
            return Err(invalid(format!(
                "sequence {} は既に在る",
                self.new_sequence
            )));
        }
        let source = self.source_cuts(project)?;
        if source.len() != self.cuts.len() {
            return Err(invalid(format!(
                "カットの ID の数 {} が元の {} と合わない",
                self.cuts.len(),
                source.len()
            )));
        }
        let cuts = source
            .iter()
            .zip(&self.cuts)
            .map(|(cut, id)| self.copy(cut, id))
            .collect();
        project.sequences.push(Sequence {
            id: self.new_sequence.clone(),
            cuts,
            origin: Some(SequenceOrigin {
                sequence: self.sequence.clone(),
                relation: self.relation,
            }),
        });
        Ok(RemoveSequence {
            sequence: self.new_sequence.clone(),
        }
        .into())
    }
}
