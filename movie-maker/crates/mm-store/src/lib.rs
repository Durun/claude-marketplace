//! プロジェクトの保存。正本は SQLite の 1 ファイル（`project.mmdb`）で、
//! プロジェクトのデータの行と操作の記録を 1 つのトランザクションで書く。
//!
//! - [`Store`]: 開く、書く、取り消す、バックアップ、検索、`versions/` の書き出しと取り込み。
//! - 操作の記録は `mm-ops` の [`OpRecord`] と [`TxnRecord`] の形で表に書く。取り消しとやり直しも
//!   逆操作（元の操作）を適用したトランザクションとして同じ表に残り、バージョンは操作の数だけ進む。
//!   操作を適用して書くまでは [`Store::apply_txn`]、[`Store::undo_last`]、[`Store::redo_last`] が受け持つ。
//! - 書き手は 1 プロセスだけ（OS のファイルロック）。ほかのプロセスは読み取りだけで開く。

mod backup;
mod docio;
mod edit;
mod export;
mod files;
mod history;
mod lock;
mod open;
mod records;
mod schema;
mod store;
mod sync;
mod wal;

pub use backup::{format_timestamp, parse_timestamp, select_backups_to_delete, BackupFile};
pub use export::{ExportReport, VERSIONS_DIR};
pub use files::{load_json, load_project, store_dir, DB_FILE};
pub use history::fold_history;
pub use lock::LockInfo;
pub use records::{
    Actor, OpRecord, StoredOp, StoredTxn, TxnInput, TxnKind, TxnRecord, UndoPlan, VersionRow,
};
pub use schema::{APPLICATION_ID, SCHEMA_VERSION};
pub use store::{OpenMode, SearchHit, Store, StoreWarning};
pub use sync::{detect_sync_folder, SyncFolder, SyncKind};
pub use wal::uncommitted_frames;

use mm_types::MmError;

/// rusqlite の失敗を `MmError` に変換する。壊れたファイルは `Corrupt`、そのほかは `Internal`。
pub(crate) fn sql_err(e: rusqlite::Error) -> MmError {
    use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
    if let rusqlite::Error::SqliteFailure(f, _) = &e {
        if matches!(f.code, DatabaseCorrupt | NotADatabase) {
            return MmError::Corrupt(format!("SQLite: {e}"));
        }
    }
    MmError::Internal(format!("SQLite: {e}"))
}

/// `rusqlite::Result` を `mm_types::Result` に変換する拡張。
pub(crate) trait SqlExt<T> {
    fn sql(self) -> mm_types::Result<T>;
}

impl<T> SqlExt<T> for rusqlite::Result<T> {
    fn sql(self) -> mm_types::Result<T> {
        self.map_err(sql_err)
    }
}
