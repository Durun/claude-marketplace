//! ノード。ノードの種類の名前とバージョン、パラメータ、入力。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use mm_types::{AssetId, ParamValue, Rgba};

use crate::param::Param;

/// 入れ子のコンポジションのノードの種類の名前。パラメータ [`NESTED_PARAM`] に中のコンポジションの ID（文字）。
pub const NESTED_KIND: &str = "composition";
/// 入れ子のコンポジションの ID を持つパラメータの名前。
pub const NESTED_PARAM: &str = "composition";
/// マグネティックシーケンスのノードの種類の名前。パラメータ [`SEQUENCE_PARAM`] に sequence の ID（文字）。
pub const SEQUENCE_KIND: &str = "sequence";
/// sequence の ID を持つパラメータの名前。
pub const SEQUENCE_PARAM: &str = "sequence";
/// 撮影素材のノードの種類の名前。パラメータ [`FOOTAGE_PARAM`] に素材の参照。
pub const FOOTAGE_KIND: &str = "footage";
/// 撮影素材の参照を持つパラメータの名前。
pub const FOOTAGE_PARAM: &str = "asset";
/// 隙間に使う単色のノードの種類の名前。
pub const GAP_KIND: &str = "solid";
/// 単色の色のパラメータの名前。
const SOLID_COLOR_PARAM: &str = "color";
/// 基本のノードの種類のバージョン（`mm-nodes-basic` の `KIND_VERSION`）。
const BASIC_KIND_VERSION: u32 = 1;

/// ノード。ノードの種類の名前とバージョン、パラメータ、入力。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Node {
    pub kind: String,
    pub version: u32,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    #[serde(default)]
    pub inputs: Vec<Node>,
}

impl Node {
    /// パラメータ無しのノードの種類。
    pub fn new(kind: impl Into<String>, version: u32) -> Self {
        Self {
            kind: kind.into(),
            version,
            params: BTreeMap::new(),
            inputs: Vec::new(),
        }
    }

    /// 固定値のパラメータを足す。
    pub fn with_param(mut self, name: impl Into<String>, value: ParamValue) -> Self {
        self.params.insert(name.into(), Param::constant(value));
        self
    }

    /// 素材 `asset` の撮影素材のノード。
    pub fn footage(asset: &AssetId) -> Self {
        Self::new(FOOTAGE_KIND, BASIC_KIND_VERSION)
            .with_param(FOOTAGE_PARAM, ParamValue::AssetRef(asset.clone()))
    }

    /// 隙間のノード。透明の単色で、音を持たない。`lift` が抜いたカットの代わりに置く。
    /// 新しいノードの種類を足さず、今のレンダラーとミキサーのまま描けて鳴らない形にする。
    pub fn gap() -> Self {
        Self::new(GAP_KIND, BASIC_KIND_VERSION)
            .with_param(SOLID_COLOR_PARAM, ParamValue::Color(Rgba::TRANSPARENT))
    }

    /// 隙間のノードか（[`Node::gap`] の形）。Timeline が空きとして描くのに使う。
    pub fn is_gap(&self) -> bool {
        *self == Self::gap()
    }

    /// 文字のパラメータの固定値。入れ子と sequence の参照を読むのに使う。
    pub fn text_param(&self, name: &str) -> Option<&str> {
        self.params.get(name).and_then(|p| p.value.as_text())
    }

    /// 自分と入力の全部を深さ優先で巡る。
    pub fn descendants(&self) -> impl Iterator<Item = &Node> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.inputs.iter());
            Some(node)
        })
    }
}
