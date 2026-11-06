//! `addRenderTarget`。レンダーターゲットを足す。

use serde::{Deserialize, Serialize};

use mm_doc::{Project, RenderTarget};
use mm_types::Result;

use crate::op::{Apply, Op};
use crate::ops::{invalid, RemoveRenderTarget};
use crate::touches::Touches;

/// レンダーターゲットを足す。無いコンポジションは `Project::validate` が拒否する。
/// `index` はレンダーターゲットの並びの位置で、無ければ末尾。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddRenderTarget {
    pub target: RenderTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

impl Apply for AddRenderTarget {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.target.id);
        t.add(&self.target.composition);
        if self.index.is_some() {
            // 位置を指す足し方は、並びの長さに頼るので、ほかのレンダーターゲットの足し引きと順が入れ替わると結果が変わる。
            for other in &project.render_targets {
                t.add(&other.id);
            }
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        if project.render_target(&self.target.id).is_ok() {
            return Err(invalid(format!(
                "レンダーターゲット {} は既に在る",
                self.target.id
            )));
        }
        let index = self.index.unwrap_or(project.render_targets.len());
        if index > project.render_targets.len() {
            return Err(invalid(format!(
                "レンダーターゲットの位置 {index} が {} を超える",
                project.render_targets.len()
            )));
        }
        project.render_targets.insert(index, self.target.clone());
        Ok(RemoveRenderTarget {
            target: self.target.id.clone(),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_types::Id;

    fn project(ids: &[&str]) -> Project {
        let mut p = Project::new();
        let proto = RenderTarget {
            id: Id::new("proto"),
            composition: Id::new("main"),
            width: 16,
            height: 9,
            fps: mm_types::Rational::integer(30),
            codec: "h264".into(),
            output_path: "out.mp4".into(),
            loudness_lufs: None,
            delivery: mm_doc::Delivery::default(),
        };
        p.render_targets = ids
            .iter()
            .map(|id| RenderTarget {
                id: Id::new(*id),
                ..proto.clone()
            })
            .collect();
        p
    }

    fn ids(p: &Project) -> Vec<&str> {
        p.render_targets.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn 外したレンダーターゲットは逆操作で元の位置に戻る() {
        let mut p = project(&["z", "a", "m"]);
        let inverse = RemoveRenderTarget {
            target: Id::new("a"),
        }
        .apply(&mut p)
        .unwrap();
        assert_eq!(ids(&p), ["z", "m"]);
        let Op::AddRenderTarget(add) = inverse else {
            panic!("逆操作は addRenderTarget");
        };
        add.apply(&mut p).unwrap();
        assert_eq!(ids(&p), ["z", "a", "m"]);
    }

    #[test]
    fn 位置を指して足し_位置が末尾を超えるなら拒否し_省けば末尾() {
        let mut p = project(&["z", "m"]);
        let target = project(&["a"]).render_targets.remove(0);
        let at = |index| AddRenderTarget {
            target: target.clone(),
            index,
        };
        assert!(at(Some(3)).apply(&mut p).is_err());
        assert_eq!(ids(&p), ["z", "m"]);
        at(Some(1)).apply(&mut p).unwrap();
        assert_eq!(ids(&p), ["z", "a", "m"]);
        let mut q = project(&["z"]);
        at(None).apply(&mut q).unwrap();
        assert_eq!(ids(&q), ["z", "a"]);
    }
}
