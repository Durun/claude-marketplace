//! プロジェクトの保存。正本は SQLite の 1 ファイル（`project.mmdb`）で、
//! 文書の行と命令の記録を 1 つのトランザクションで書く。
//!
//! - [`Store`]: 開く、書く、取り消す、バックアップ、検索、`versions/` の書き出しと取り込み。
//! - 命令の型は持たない。[`OpRecord`] は命令と逆命令を JSON で持ち、CLI の段で `mm-ops` の型とつなぐ。
//! - 書き手は 1 プロセスだけ（OS のファイルロック）。ほかのプロセスは読み取りだけで開く。

mod backup;
mod docio;
mod export;
mod history;
mod lock;
mod records;
mod schema;
mod store;
mod sync;

pub use backup::{format_timestamp, parse_timestamp, select_backups_to_delete, BackupFile};
pub use export::{ExportReport, VERSIONS_DIR};
pub use lock::LockInfo;
pub use records::{
    Actor, CommitInfo, OpRecord, StoredOp, StoredTxn, TxnInput, TxnState, UndoPlan, VersionRow,
};
pub use schema::{APPLICATION_ID, SCHEMA_VERSION};
pub use store::{OpenMode, SearchHit, Store, StoreWarning};
pub use sync::{detect_sync_folder, SyncFolder, SyncKind};

use mm_types::MmError;

/// rusqlite の失敗を `MmError` に写す。壊れたファイルは `Corrupt`、そのほかは `Internal`。
pub(crate) fn sql_err(e: rusqlite::Error) -> MmError {
    use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
    if let rusqlite::Error::SqliteFailure(f, _) = &e {
        if matches!(f.code, DatabaseCorrupt | NotADatabase) {
            return MmError::Corrupt(format!("SQLite: {e}"));
        }
    }
    MmError::Internal(format!("SQLite: {e}"))
}

/// `rusqlite::Result` を `mm_types::Result` に写す拡張。
pub(crate) trait SqlExt<T> {
    fn sql(self) -> mm_types::Result<T>;
}

impl<T> SqlExt<T> for rusqlite::Result<T> {
    fn sql(self) -> mm_types::Result<T> {
        self.map_err(sql_err)
    }
}
