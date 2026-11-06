//! `touches`。操作が読む・書く所の ID の集合。
//!
//! プロジェクトのデータのバージョンをまたいだ衝突の判定と、順の入れ替えの判定に使う。重ならない 2 つの操作は、
//! 順を入れ替えても同じプロジェクトのデータになる（性質テストが守る）。ID は文字列で、保存の表には列として書く。

use std::collections::BTreeSet;

use mm_types::Id;

/// 触る所の ID の集合。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Touches(BTreeSet<String>);

impl Touches {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 1 つの ID から。
    pub fn of<T>(id: &Id<T>) -> Self {
        let mut t = Self::new();
        t.add(id);
        t
    }

    /// ID を足す。
    pub fn add<T>(&mut self, id: &Id<T>) -> &mut Self {
        self.0.insert(id.as_str().to_owned());
        self
    }

    /// 文字列の ID を足す。
    pub fn add_str(&mut self, id: &str) -> &mut Self {
        self.0.insert(id.to_owned());
        self
    }

    /// 別の集合を全部足す。
    pub fn extend(&mut self, other: &Touches) -> &mut Self {
        self.0.extend(other.0.iter().cloned());
        self
    }

    /// ID を含むか。
    pub fn contains(&self, id: &str) -> bool {
        self.0.contains(id)
    }

    /// 重なるか。
    pub fn overlaps(&self, other: &Touches) -> bool {
        self.0.iter().any(|id| other.0.contains(id))
    }

    /// 重なる所。
    pub fn intersection(&self, other: &Touches) -> Touches {
        Touches(self.0.intersection(&other.0).cloned().collect())
    }

    /// 空か。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// ID の列（保存の表に書く形）。
    pub fn to_vec(&self) -> Vec<String> {
        self.0.iter().cloned().collect()
    }

    /// 中の集合。
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

impl FromIterator<String> for Touches {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}
