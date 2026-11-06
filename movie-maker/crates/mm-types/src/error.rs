//! 全クレートで共通のエラー。API クレートはこの型を返し、実装の詳細は文字列にする。

use std::time::Duration;

/// movie-maker のエラー。
#[derive(Debug, thiserror::Error)]
pub enum MmError {
    /// 引数が契約に合わない。呼ぶ側の誤り。
    #[error("引数が不正: {0}")]
    InvalidArgument(String),
    /// 名前や ID で引いたものが無い。
    #[error("見つからない: {0}")]
    NotFound(String),
    /// この実装が対応していない形式や操作。
    #[error("未対応: {0}")]
    Unsupported(String),
    /// 期限までに終わらなかった。
    #[error("{what} が {limit:?} で時間切れ")]
    Timeout { what: String, limit: Duration },
    /// 入力のファイルやストリームが壊れている。クラッシュせずにこれを返す。
    #[error("入力が壊れている: {0}")]
    Corrupt(String),
    /// 別プロセスのホストがクラッシュした、または応答しない。
    #[error("ホストがクラッシュした: {0}")]
    HostCrashed(String),
    /// ファイルの読み書き。
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON の読み書き。
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// 実装の内部の不整合。利用者には直せない。
    #[error("内部エラー: {0}")]
    Internal(String),
}

/// 共通の `Result`。
pub type Result<T> = std::result::Result<T, MmError>;
