//! `deleteWords`。文字起こしの単語を消す・戻す。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use mm_doc::{Project, WordId};
use mm_types::{AssetId, Result};

use crate::op::{Apply, Op};
use crate::ops::bins::update_edit;
use crate::ops::invalid;
use crate::touches::Touches;

/// 素材 `asset` の単語 `words` を消す（`restore` で戻す）。消した単語は検索と表示から外れ、本文の表は変わらない。
/// 既に消えている単語を消す、消えていない単語を戻す、同じ単語を 2 回挙げるのは拒否する（逆操作がちょうど戻すため）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteWords {
    pub asset: AssetId,
    pub words: Vec<WordId>,
    #[serde(default)]
    pub restore: bool,
}

impl Apply for DeleteWords {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.asset);
        for w in &self.words {
            t.add(w);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if self.words.is_empty() {
            return Err(invalid("消す単語が無い"));
        }
        let mut seen = HashSet::new();
        let want = !self.restore;
        for word in &self.words {
            if !seen.insert(word) {
                return Err(invalid(format!("単語 {word} を 2 回挙げている")));
            }
            let old = update_edit(project, &self.asset, word, |e| e.deleted = want)?;
            if old.deleted == want {
                return Err(invalid(format!("単語 {word} は既にその状態")));
            }
        }
        Ok(DeleteWords {
            asset: self.asset.clone(),
            words: self.words.clone(),
            restore: want,
        }
        .into())
    }
}
