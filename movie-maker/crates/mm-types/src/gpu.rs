//! 使う GPU の識別。複数の GPU がある PC で、描画器・デコーダー・別プロセスのホストが
//! 同じアダプターを選ぶために `mm-runtime` の組み立てで 1 回決めて渡す。

use serde::{Deserialize, Serialize};

/// GPU のアダプターの識別子。実装（wgpu）に依存しない値だけを持つ。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GpuAdapterId {
    /// PCI のベンダー ID。
    pub vendor: u32,
    /// PCI のデバイス ID。
    pub device: u32,
    /// ドライバが返す名前（`NVIDIA GeForce RTX 5090`）。
    pub name: String,
    /// D3D12 の LUID。別プロセスのホストが同じアダプターを開くのに使う。取れない裏では `None`。
    #[serde(default)]
    pub luid: Option<u64>,
}

impl GpuAdapterId {
    /// 同じアダプターか。LUID が両方にあればそれで、無ければベンダー・デバイス・名前で決める。
    pub fn matches(&self, other: &Self) -> bool {
        match (self.luid, other.luid) {
            (Some(a), Some(b)) => a == b,
            _ => {
                self.vendor == other.vendor
                    && self.device == other.device
                    && self.name == other.name
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luid_があればそれで決める() {
        let a = GpuAdapterId {
            vendor: 1,
            device: 2,
            name: "x".into(),
            luid: Some(10),
        };
        let b = GpuAdapterId {
            luid: Some(11),
            ..a.clone()
        };
        assert!(!a.matches(&b));
        let c = GpuAdapterId {
            luid: None,
            ..a.clone()
        };
        assert!(a.matches(&c));
    }
}
