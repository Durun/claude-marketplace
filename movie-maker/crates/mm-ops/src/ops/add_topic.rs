//! `addTopic`。話題を足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, Topic};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveTopic};
use crate::touches::Touches;

/// 話題（文字起こしの区間に見出しを付けた素材の中の範囲）を足す。同じ ID の選別か話題が在れば拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddTopic {
    pub topic: Topic,
}

impl Apply for AddTopic {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.topic.id);
        t.add(&self.topic.asset);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let id = &self.topic.id;
        if project.topic(id).is_ok() {
            return Err(invalid(format!("話題 {id} は既に在る")));
        }
        let list = &mut project.bins.topics;
        let pos = list.partition_point(|t| t.id < *id);
        list.insert(pos, self.topic.clone());
        Ok(RemoveTopic { topic: id.clone() }.into())
    }
}
