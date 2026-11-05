//! 動画オブジェクト。種類の名前と版、パラメータ、入力。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use mm_types::ParamValue;

use crate::param::Param;

/// 入れ子のコンポジションの種類の名前。パラメータ [`NESTED_PARAM`] に中のコンポジションの ID（文字）。
pub const NESTED_KIND: &str = "composition";
/// 入れ子のコンポジションの ID を持つパラメータの名前。
pub const NESTED_PARAM: &str = "composition";
/// 磁気の並びの種類の名前。パラメータ [`SEQUENCE_PARAM`] に sequence の ID（文字）。
pub const SEQUENCE_KIND: &str = "sequence";
/// sequence の ID を持つパラメータの名前。
pub const SEQUENCE_PARAM: &str = "sequence";

/// 動画オブジェクト。種類の名前と版、パラメータ、入力。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub kind: String,
    pub version: u32,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    #[serde(default)]
    pub inputs: Vec<Node>,
}

impl Node {
    /// パラメータ無しの種類。
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
