//! 操作の層のエラー。文書の誤り（[`MmError`]）に、衝突とロックを足す。

use mm_types::MmError;

use crate::touches::Touches;

/// 操作を当てられない理由。
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    /// 元にした版の後に、同じ所を触る操作が当たっている。呼ぶ側が今の文書で作り直す。
    #[error("版 {base} の後の操作と触る所が重なる: {touches:?}")]
    Conflict { base: u64, touches: Touches },
    /// 元にした版が今の版より先（別の文書の版を渡している）。
    #[error("元にした版 {base} が今の版 {current} より先")]
    FutureBase { base: u64, current: u64 },
    /// ロックの付いた所に Claude の入口から触ろうとした。
    #[error("ロックの付いた所に触れない: {0:?}")]
    Locked(Touches),
    /// 操作の検証・適用で見つかった文書の誤り。
    #[error(transparent)]
    Doc(#[from] MmError),
}

/// 操作の層の `Result`。
pub type OpResult<T> = std::result::Result<T, OpError>;
