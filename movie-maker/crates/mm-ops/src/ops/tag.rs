//! `tag`。選別か話題にタグを付ける・外す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::bins::item_mut;
use crate::ops::invalid;
use crate::touches::Touches;

/// 選別か話題（`target` の ID）にタグ `tag` を付ける（`remove` で外す）。タグは辞書順に並ぶ。
/// 付いているタグを付ける、付いていないタグを外すのは拒否する（逆操作がちょうど戻すため）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tag {
    /// 選別か話題の ID。
    pub target: String,
    pub tag: String,
    #[serde(default)]
    pub remove: bool,
}

impl Apply for Tag {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::new();
        t.add_str(&self.target);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let (_, tags) = item_mut(project, &self.target)?;
        match (tags.binary_search(&self.tag), self.remove) {
            (Err(pos), false) => tags.insert(pos, self.tag.clone()),
            (Ok(pos), true) => {
                tags.remove(pos);
            }
            (Ok(_), false) => return Err(invalid(format!("タグ {} は付いている", self.tag))),
            (Err(_), true) => return Err(invalid(format!("タグ {} は付いていない", self.tag))),
        }
        Ok(Tag {
            target: self.target.clone(),
            tag: self.tag.clone(),
            remove: !self.remove,
        }
        .into())
    }
}
