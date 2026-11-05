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
    /// D3D12 の LUID。起動のたびに変わるので設定には残さず、その回のホストへの受け渡しにだけ使う。
    /// 取れない裏では `None`。
    #[serde(default)]
    pub luid: Option<u64>,
}

impl GpuAdapterId {
    /// 同じ機種か。ベンダー・デバイス・名前で決める。LUID は見ないので、設定に保存した識別子を
    /// 再起動後の PC の一覧と照合できる。この関係は推移的。
    pub fn matches(&self, other: &Self) -> bool {
        self.vendor == other.vendor && self.device == other.device && self.name == other.name
    }

    /// 同じ機種で、LUID が両方にあれば LUID も等しいか。同じ機種を 2 枚挿した PC で、
    /// 動いている回の中の 1 枚を指すのに使う。
    pub fn is_same_instance(&self, other: &Self) -> bool {
        self.matches(other)
            && match (self.luid, other.luid) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(luid: Option<u64>) -> GpuAdapterId {
        GpuAdapterId {
            vendor: 1,
            device: 2,
            name: "x".into(),
            luid,
        }
    }

    #[test]
    fn 機種の照合は_luid_を見ない() {
        // 再起動で LUID が変わっても、保存した識別子は同じ GPU とみなす。
        assert!(adapter(Some(10)).matches(&adapter(Some(11))));
        assert!(adapter(Some(10)).matches(&adapter(None)));
        assert!(adapter(None).matches(&adapter(Some(10))));
        let other = GpuAdapterId {
            device: 3,
            ..adapter(None)
        };
        assert!(!adapter(None).matches(&other));
    }

    #[test]
    fn 同じ回の中では_luid_で見分ける() {
        assert!(adapter(Some(10)).is_same_instance(&adapter(Some(10))));
        assert!(!adapter(Some(10)).is_same_instance(&adapter(Some(11))));
        assert!(adapter(Some(10)).is_same_instance(&adapter(None)));
    }
}
