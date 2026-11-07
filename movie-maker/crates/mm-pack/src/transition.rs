//! `transition` 手順の展開と、`transitions/*.wgsl` の読み込み。
//!
//! パックの `transitions/<名前>.wgsl` を `transition.wgsl` のノードのコードにして、カットの頭に `setTransition` で置く。
//! `_` で始まるファイルは全部のトランジションに共通のコードで、名前の順にトランジションのコードの前に付ける。
//! コードはノードのパラメータにコピーするので、プロジェクトはパックのフォルダーが無くても同じに描ける。

use std::collections::BTreeMap;
use std::path::Path;

use mm_doc::{Node, Transition};
use mm_types::{MmError, ParamValue, RationalTime, Result};
use serde_json::{json, Value};

use crate::args::{parse_time, Env};
use crate::macros::TransitionStep;
use crate::pack::Pack;

/// `transition.wgsl` の種類の名前とバージョン（`mm-nodes-basic` の `TransitionWgslKind`）。
const TRANSITION_KIND: &str = "transition.wgsl";
const TRANSITION_KIND_VERSION: u32 = 1;
/// 共通のコードのファイルの名前の頭。
const SHARED_PREFIX: &str = "_";

/// パックのトランジション。名前 → 共通のコードを前に付けた WGSL。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Transitions {
    pub code: BTreeMap<String, String>,
}

impl Transitions {
    /// `transitions/` を読む。フォルダーが無ければ空。
    pub(crate) fn load(dir: &Path) -> Result<Self> {
        if !dir.is_dir() {
            return Ok(Self::default());
        }
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| MmError::NotFound(format!("{}: {e}", dir.display())))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "wgsl"))
            .collect();
        paths.sort();
        let mut shared = String::new();
        let mut own = BTreeMap::new();
        for path in paths {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let text = std::fs::read_to_string(&path)
                .map_err(|e| MmError::NotFound(format!("{}: {e}", path.display())))?;
            if name.starts_with(SHARED_PREFIX) {
                shared.push_str(&text);
                shared.push('\n');
            } else {
                own.insert(name.to_string(), text);
            }
        }
        let code = own
            .into_iter()
            .map(|(name, text)| (name, format!("{shared}{text}")))
            .collect();
        Ok(Self { code })
    }

    /// トランジションの名前の一覧。
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.code.keys().map(String::as_str)
    }
}

/// `transition` 手順を `setTransition` 1 つにする。
pub(crate) fn transition_op(pack: &Pack, env: &Env, step: &TransitionStep) -> Result<Value> {
    let name = env.fill_text(&step.name)?;
    let code = pack.transitions.code.get(&name).ok_or_else(|| {
        MmError::InvalidArgument(format!(
            "transitions/{name}.wgsl がパック {} に無い（あるのは {}）",
            pack.manifest.name,
            pack.transitions.names().collect::<Vec<_>>().join(", ")
        ))
    })?;
    let cut = env.fill_text(&step.cut)?;
    let duration = parse_time(&env.fill_text(&step.dur)?)?;
    if duration <= RationalTime::ZERO {
        return Err(MmError::InvalidArgument(format!(
            "{cut}: トランジションの長さは正にする"
        )));
    }
    let mut node = Node::new(TRANSITION_KIND, TRANSITION_KIND_VERSION)
        .with_param("code", ParamValue::Text(code.clone()));
    for (key, raw) in &step.params {
        let text = env.fill_text(raw)?;
        let value: f64 = text.trim().parse().map_err(|_| {
            MmError::InvalidArgument(format!("transition の {key} は数にする: {text}"))
        })?;
        node = node.with_param(key, ParamValue::Number(value));
    }
    let transition = Transition { node, duration };
    Ok(json!({"op": "setTransition", "cut": cut, "transition": serde_json::to_value(transition)?}))
}
