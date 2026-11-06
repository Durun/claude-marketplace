//! `fixTranscript`。文字起こしの単語の文字を直す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, WordId};
use mm_types::{AssetId, Result};

use crate::op::{Apply, Op};
use crate::ops::bins::update_edit;
use crate::touches::Touches;

/// 素材 `asset` の単語 `word` の文字を `text` に直す。`None` で元の文字に戻す。本文の表は変わらない。
/// 複数の単語にまたがる直しは、1 つ目に全体を、残りに空の文字を置く（時刻は単語のまま）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixTranscript {
    pub asset: AssetId,
    pub word: WordId,
    pub text: Option<String>,
}

impl Apply for FixTranscript {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.asset);
        t.add(&self.word);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let text = self.text.clone();
        let old = update_edit(project, &self.asset, &self.word, |e| e.text = text)?;
        Ok(FixTranscript {
            asset: self.asset.clone(),
            word: self.word.clone(),
            text: old.text,
        }
        .into())
    }
}
