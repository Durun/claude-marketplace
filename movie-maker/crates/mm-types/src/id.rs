//! 型つきの ID。プロジェクトのデータの中では文字列で持ち、型タグでレイヤーと素材の取り違えを防ぐ。

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// `T` は何の ID かを表す型タグ。文字列の内容は決定的な JSON に出るので、生成の規則は mm-ops が決める。
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct Id<T> {
    value: String,
    #[serde(skip)]
    marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// 文字列から。
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            marker: PhantomData,
        }
    }

    /// 文字列の内容。
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.value)
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.cmp(&other.value)
    }
}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

/// 素材のマーカー。
#[derive(Debug)]
pub enum AssetMarker {}
/// コンポジションのマーカー。
#[derive(Debug)]
pub enum CompositionMarker {}
/// レイヤーのマーカー。
#[derive(Debug)]
pub enum LayerMarker {}
/// ノードのマーカー。
#[derive(Debug)]
pub enum NodeMarker {}
/// レンダーターゲットのマーカー。
#[derive(Debug)]
pub enum RenderTargetMarker {}

/// 素材の ID。
pub type AssetId = Id<AssetMarker>;
/// コンポジションの ID。
pub type CompositionId = Id<CompositionMarker>;
/// レイヤーの ID。
pub type LayerId = Id<LayerMarker>;
/// ノードの ID。
pub type NodeId = Id<NodeMarker>;
/// レンダーターゲットの ID。
pub type RenderTargetId = Id<RenderTargetMarker>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_はそのままの文字列() {
        let id: AssetId = Id::new("a1");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"a1\"");
        let back: AssetId = serde_json::from_str("\"a1\"").unwrap();
        assert_eq!(id, back);
    }
}
