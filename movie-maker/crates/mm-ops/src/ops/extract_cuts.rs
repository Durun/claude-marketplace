//! `extractCuts`。続いたカットの列を抜く（後ろが詰まる）。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, SequenceId};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_referrers, cut_refs_reach, invalid, InsertCuts};
use crate::touches::Touches;

/// `sequence` の `index` から `count` 個のカットを抜く。接続しているレイヤーがあれば `Project::validate` が拒否する。
/// 範囲の選択の削除と `insertCuts`・`insertTopics` の逆操作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractCuts {
    pub sequence: SequenceId,
    pub index: usize,
    pub count: usize,
}

impl ExtractCuts {
    fn range(&self, len: usize) -> Result<std::ops::Range<usize>> {
        let end = self.index.checked_add(self.count).filter(|e| *e <= len);
        match end {
            Some(end) if self.count > 0 => Ok(self.index..end),
            _ => Err(invalid(format!(
                "カットの範囲 {}+{} がシーケンスの {len} 個の中に無い",
                self.index, self.count
            ))),
        }
    }
}

impl Apply for ExtractCuts {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let seq = project.sequence(&self.sequence)?;
        let range = self.range(seq.cuts.len())?;
        let mut t = Touches::of(&self.sequence);
        for cut in &seq.cuts[range.clone()] {
            t.add(&cut.id);
            cut_refs_reach(project, cut, &mut t);
            cut_referrers(project, &cut.id, &mut t);
        }
        let around = range.start.checked_sub(1).into_iter().chain([range.end]);
        for cut in around.filter_map(|i| seq.cuts.get(i)) {
            t.add(&cut.id);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let seq = project.sequence_mut(&self.sequence)?;
        let range = self.range(seq.cuts.len())?;
        let cuts = seq.cuts.drain(range).collect();
        Ok(InsertCuts {
            sequence: self.sequence.clone(),
            index: self.index,
            cuts,
        }
        .into())
    }
}
