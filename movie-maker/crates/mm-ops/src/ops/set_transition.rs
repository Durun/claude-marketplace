//! `setTransition`。カットの頭（前のカットとの境）のトランジションを置く・差し替える・外す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Project, Transition};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{cut_refs, invalid, node_refs};
use crate::touches::Touches;

/// カット `cut` の `transition_in` を `transition` にする（`None` で外す）。種類（`node.kind`）、パラメータ、長さの
/// 変更も、この操作で丸ごと差し替える。sequence の先頭のカットに置いたトランジションは描かない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTransition {
    pub cut: CutId,
    #[serde(default)]
    pub transition: Option<Transition>,
}

impl Apply for SetTransition {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let (_, cut) = project.cut(&self.cut)?;
        let mut t = Touches::of(&self.cut);
        cut_refs(cut, &mut t);
        if let Some(tr) = &self.transition {
            node_refs(&tr.node, &mut t);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if let Some(tr) = &self.transition {
            // 長さ 0 は validate が通す形（描かない）なので、逆操作で戻せるよう負だけを弾く。
            if tr.duration.is_negative() {
                return Err(invalid(format!(
                    "カット {} のトランジションの長さが負",
                    self.cut
                )));
            }
        }
        let cut = project.cut_mut(&self.cut)?;
        let old = std::mem::replace(&mut cut.transition_in, self.transition.clone());
        Ok(SetTransition {
            cut: self.cut.clone(),
            transition: old,
        }
        .into())
    }
}
