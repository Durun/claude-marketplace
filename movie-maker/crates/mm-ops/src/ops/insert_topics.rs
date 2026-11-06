//! `insertTopics`。選んだ話題の列から、シーケンスのカットを一度に作る。

use serde::{Deserialize, Serialize};

use mm_doc::{Cut, CutId, Node, Project, SequenceId, TopicId};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::insert_cuts::splice;
use crate::ops::{cuts_around_gap, invalid, ExtractCuts};
use crate::touches::Touches;

/// `topics` の順に、話題の素材の範囲を切り出したカット（撮影素材のノード、等速）を `sequence` の `index` に差し込む。
/// `index` を省くと末尾。カットの ID は `cuts` で話題と同じ数だけ渡す（呼び出し側が ID を決めると、逆操作とやり直しで同じ ID に戻る）。
/// 1 つの操作なので、1 回の取り消しで全部のカットが抜ける。逆操作は `extractCuts`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertTopics {
    pub sequence: SequenceId,
    #[serde(default)]
    pub index: Option<usize>,
    pub topics: Vec<TopicId>,
    pub cuts: Vec<CutId>,
}

impl InsertTopics {
    fn at(&self, project: &Project) -> Result<usize> {
        let len = project.sequence(&self.sequence)?.cuts.len();
        Ok(self.index.unwrap_or(len))
    }

    fn build(&self, project: &Project) -> Result<Vec<Cut>> {
        if self.topics.len() != self.cuts.len() {
            return Err(invalid(format!(
                "話題 {} 個にカットの ID が {} 個",
                self.topics.len(),
                self.cuts.len()
            )));
        }
        self.topics
            .iter()
            .zip(&self.cuts)
            .map(|(topic, id)| {
                let t = project.topic(topic)?;
                let source = Node::footage(&t.asset);
                Ok(Cut::new(id.as_str(), source, t.in_point, t.out_point))
            })
            .collect()
    }
}

impl Apply for InsertTopics {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.sequence);
        for id in &self.cuts {
            t.add(id);
        }
        for topic in &self.topics {
            t.add(topic);
            if let Ok(found) = project.topic(topic) {
                t.add(&found.asset);
            }
        }
        let index = self.at(project)?;
        cuts_around_gap(project.sequence(&self.sequence)?, index, &mut t);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let index = self.at(project)?;
        let cuts = self.build(project)?;
        splice(project, &self.sequence, index, &cuts)?;
        Ok(ExtractCuts {
            sequence: self.sequence.clone(),
            index,
            count: cuts.len(),
        }
        .into())
    }
}
