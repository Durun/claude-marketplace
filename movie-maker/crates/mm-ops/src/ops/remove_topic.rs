//! `removeTopic`。話題を外す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, TopicId};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::AddTopic;
use crate::touches::Touches;

/// 話題を外す（`addTopic` の逆操作）。話題から作ったカットは残る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveTopic {
    pub topic: TopicId,
}

impl Apply for RemoveTopic {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let topic = project.topic(&self.topic)?;
        let mut t = Touches::of(&topic.id);
        t.add(&topic.asset);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        project.topic(&self.topic)?;
        let list = &mut project.bins.topics;
        let pos = list
            .iter()
            .position(|t| t.id == self.topic)
            .expect("topic が引けた");
        Ok(AddTopic {
            topic: list.remove(pos),
        }
        .into())
    }
}
