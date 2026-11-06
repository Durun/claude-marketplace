//! `setReframe`。sequence のレイヤーのカット 1 つの再フレーミングを置く・外す。

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, CutReframe, Project, Transform, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_types::{LayerId, Result};

use crate::op::{Apply, Op};
use crate::ops::invalid;
use crate::touches::Touches;

/// sequence のレイヤー `layer` で、カット `cut` の画像に掛ける変形を `transform` にする（`None` で外す）。
/// キーフレームの時刻は素材の時刻。キーフレームは `setKeyframe` の `reframe.<カット>.position` などでも置ける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetReframe {
    pub layer: LayerId,
    pub cut: CutId,
    #[serde(default)]
    pub transform: Option<Transform>,
}

impl SetReframe {
    /// 置く・外すカットがレイヤーの sequence に在ること。
    fn check(&self, project: &Project) -> Result<()> {
        let (_, layer) = project.layer(&self.layer)?;
        if layer.source.kind != SEQUENCE_KIND {
            return Err(invalid(format!(
                "レイヤー {} は sequence のレイヤーでない",
                self.layer
            )));
        }
        let seq = layer.source.text_param(SEQUENCE_PARAM).unwrap_or("");
        let (owner, _) = project.cut(&self.cut)?;
        if owner.id.as_str() != seq {
            return Err(invalid(format!(
                "カット {} はレイヤー {} の sequence {seq} に無い",
                self.cut, self.layer
            )));
        }
        Ok(())
    }
}

impl Apply for SetReframe {
    fn touches(&self, _: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.layer);
        t.add(&self.cut);
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        // 外すときも検査する。消えたカットの項目を外すと、逆操作（項目を置き直す）が検査で通らなくなるため。
        self.check(project)?;
        let cuts = &mut project.layer_mut(&self.layer)?.framing.cuts;
        let old = cuts
            .iter()
            .position(|r| r.cut == self.cut)
            .map(|i| cuts.remove(i));
        if let Some(transform) = &self.transform {
            cuts.push(CutReframe {
                cut: self.cut.clone(),
                transform: transform.clone(),
            });
            cuts.sort_by(|a, b| a.cut.cmp(&b.cut));
        }
        Ok(SetReframe {
            layer: self.layer.clone(),
            cut: self.cut.clone(),
            transform: old.map(|r| r.transform),
        }
        .into())
    }
}
