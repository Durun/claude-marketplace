//! 動画オブジェクトの種類の登録簿。種類を足すのは実装のクレートの追加で、ここに 1 行足す。

use std::collections::BTreeMap;
use std::sync::Arc;

use mm_node_api::{NodeKind, NodeRegistry};
use mm_types::{MmError, Result};

/// 登録簿。同じ名前の二重登録は断る。
#[derive(Default, Clone)]
pub struct Registry {
    kinds: BTreeMap<&'static str, Arc<dyn NodeKind>>,
}

impl Registry {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 種類を登録する。同じ名前があれば `InvalidArgument`。
    pub fn register(&mut self, kind: Arc<dyn NodeKind>) -> Result<()> {
        let id = kind.id();
        if self.kinds.contains_key(id) {
            return Err(MmError::InvalidArgument(format!("種類 {id} は二重登録")));
        }
        self.kinds.insert(id, kind);
        Ok(())
    }

    /// 登録されている数。
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// 空か。
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
}

impl NodeRegistry for Registry {
    fn get(&self, kind: &str) -> Option<Arc<dyn NodeKind>> {
        self.kinds.get(kind).cloned()
    }

    fn kinds(&self) -> Vec<&'static str> {
        self.kinds.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_testkit::node::SolidKind;

    #[test]
    fn 二重登録は断る() {
        let mut r = Registry::new();
        r.register(Arc::new(SolidKind)).unwrap();
        assert!(r.register(Arc::new(SolidKind)).is_err());
        assert_eq!(r.kinds(), vec!["solid"]);
    }
}
