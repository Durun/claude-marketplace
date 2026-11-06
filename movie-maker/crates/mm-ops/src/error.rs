//! 操作の層のエラー。プロジェクトのデータの誤り（[`MmError`]）に、衝突とロックを足す。

use mm_types::MmError;

use crate::touches::Touches;

/// 操作を適用できない理由。
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    /// 元にしたバージョンの後に、同じ所を触る操作が適用されている。呼ぶ側が今のプロジェクトのデータで作り直す。
    #[error("バージョン {base} の後の操作と触る所が重なる: {touches:?}")]
    Conflict { base: u64, touches: Touches },
    /// 元にしたバージョンが今のバージョンより先（別のプロジェクトのデータのバージョンを渡している）。
    #[error("元にしたバージョン {base} が今のバージョン {current} より先")]
    FutureBase { base: u64, current: u64 },
    /// ロックの付いた所に Claude の操作経路から触ろうとした。
    #[error("ロックの付いた所に触れない: {0:?}")]
    Locked(Touches),
    /// 操作の検証・適用で見つかったプロジェクトのデータの誤り。
    #[error(transparent)]
    Doc(#[from] MmError),
}

/// 操作の層の `Result`。
pub type OpResult<T> = std::result::Result<T, OpError>;
