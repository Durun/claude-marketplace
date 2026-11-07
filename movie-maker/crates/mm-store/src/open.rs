//! `Store` の開き方。作る、書き手として開く、読むだけで開く。整合の検査とスキーマの移行。

use std::fs;
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};

use mm_doc::Project;
use mm_types::{MmError, Result};

use crate::docio;
use crate::lock::{self, WriterLock};
use crate::schema::{self, MIGRATIONS};
use crate::store::{OpenMode, Store, StoreWarning, DB_FILE};
use crate::sync::detect_sync_folder;
use crate::{backup, SqlExt};

const BUSY_TIMEOUT_MS: u64 = 5000;
/// 作る途中のファイル。全部書けてから `project.mmdb` の名前にするので、途中でクラッシュしても半端な DB が残らない。
const NEW_FILE: &str = "project.mmdb.new";

impl Store {
    /// 空の DB を作ってプロジェクトのデータを入れる。すでにあれば拒否する。書き手になれなければ拒否する。
    pub fn create(dir: &Path, project: &Project, now_ms: i64) -> Result<Self> {
        fs::create_dir_all(dir)?;
        if dir.join(DB_FILE).exists() {
            return Err(MmError::InvalidArgument(format!(
                "{} はすでにある",
                dir.join(DB_FILE).display()
            )));
        }
        let lock = WriterLock::try_acquire(dir)?
            .ok_or_else(|| MmError::InvalidArgument("別のプロセスが書き手".into()))?;
        let mut project = project.clone();
        project.normalize();
        let rows = docio::rows_of(&project)?;
        write_new_db(dir, &rows, now_ms)?;
        fs::rename(dir.join(NEW_FILE), dir.join(DB_FILE))?;
        let conn = open_connection(&dir.join(DB_FILE), OpenMode::Writer)?;
        let mut store = Self::assemble(dir, conn, OpenMode::Writer, Some(lock), project, rows);
        store.last_backup_ms = now_ms;
        store.read_meta()?;
        Ok(store)
    }

    /// 開く。別のプロセスが書き手なら読み取りだけで開く。壊れていれば `Corrupt`。
    pub fn open(dir: &Path, now_ms: i64) -> Result<Self> {
        let db = existing_db(dir)?;
        let lock = WriterLock::try_acquire(dir)?;
        let mode = if lock.is_some() {
            OpenMode::Writer
        } else {
            OpenMode::ReadOnly
        };
        let mut conn = open_connection(&db, mode)?;
        check_integrity(&conn, dir)?;
        if mode == OpenMode::Writer {
            migrate_with_backup(&mut conn, dir, now_ms)?;
        } else {
            require_current_schema(&conn)?;
        }
        let mut store = Self::assemble_from_db(dir, conn, mode, lock)?;
        store.finish_open(now_ms)?;
        Ok(store)
    }

    /// 読むだけで開く。書き手のロックを取らず、移行もしないので、アプリが書いている最中でも邪魔をしない
    /// （`mm log`、`mm diff`、`mm export`、`mm render`）。
    pub fn open_read_only(dir: &Path) -> Result<Self> {
        let db = existing_db(dir)?;
        let conn = open_connection(&db, OpenMode::ReadOnly)?;
        check_integrity(&conn, dir)?;
        require_current_schema(&conn)?;
        let mut store = Self::assemble_from_db(dir, conn, OpenMode::ReadOnly, None)?;
        store.read_meta()?;
        if let Some(info) = lock::read_info(dir) {
            store
                .warnings
                .push(StoreWarning::WriterElsewhere(Some(info)));
        }
        Ok(store)
    }

    fn assemble_from_db(
        dir: &Path,
        conn: Connection,
        mode: OpenMode,
        lock: Option<WriterLock>,
    ) -> Result<Self> {
        let project = docio::read_project(&conn)?;
        let rows = docio::rows_of(&project)?;
        Ok(Self::assemble(dir, conn, mode, lock, project, rows))
    }

    fn assemble(
        dir: &Path,
        conn: Connection,
        mode: OpenMode,
        lock: Option<WriterLock>,
        project: Project,
        rows: docio::RowMap,
    ) -> Self {
        let sync_folder = detect_sync_folder(dir);
        let warnings = sync_folder
            .iter()
            .map(|s| StoreWarning::SyncFolder(s.clone()))
            .collect();
        Self {
            dir: dir.to_path_buf(),
            conn,
            mode,
            lock,
            project,
            rows,
            doc_version: 0,
            undo_pos: 0,
            warnings,
            last_backup_ms: 0,
            sync_folder,
        }
    }

    fn read_meta(&mut self) -> Result<()> {
        self.doc_version = get_meta_i64(&self.conn, "doc_version")?;
        self.undo_pos = get_meta_i64(&self.conn, "undo_pos")?;
        Ok(())
    }

    fn finish_open(&mut self, now_ms: i64) -> Result<()> {
        self.read_meta()?;
        // 前回のバックアップの時刻。無ければ作った時刻から数える。
        self.last_backup_ms = backup::list(&self.dir).first().map_or_else(
            || get_meta_i64(&self.conn, "created_at_ms"),
            |b| Ok(b.at_ms),
        )?;
        if self.mode == OpenMode::ReadOnly {
            let info = lock::read_info(&self.dir);
            self.warnings.push(StoreWarning::WriterElsewhere(info));
            return Ok(());
        }
        if !docio::stored_doc_is_current(&self.conn)? {
            backup::run(&self.conn, &self.dir, now_ms)?;
            let tx = self.conn.transaction().sql()?;
            docio::write_all(&tx, &self.rows)?;
            tx.commit().sql()?;
        }
        Ok(())
    }
}

fn existing_db(dir: &Path) -> Result<std::path::PathBuf> {
    let db = dir.join(DB_FILE);
    if !db.exists() {
        return Err(MmError::NotFound(db.display().to_string()));
    }
    Ok(db)
}

/// `project.mmdb.new` に表とプロジェクトのデータと meta を書き、閉じる。
fn write_new_db(dir: &Path, rows: &docio::RowMap, now_ms: i64) -> Result<()> {
    let path = dir.join(NEW_FILE);
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(dir.join(format!("{NEW_FILE}{suffix}")));
    }
    let mut conn = Connection::open(&path).sql()?;
    configure(&conn, OpenMode::Writer)?;
    schema::migrate(&mut conn, MIGRATIONS)?;
    let tx = conn.transaction().sql()?;
    docio::write_all(&tx, rows)?;
    set_meta(
        &tx,
        "project_id",
        &format!("{now_ms:x}-{}", std::process::id()),
    )?;
    set_meta(&tx, "created_at_ms", &now_ms.to_string())?;
    set_meta(&tx, "doc_version", "0")?;
    set_meta(&tx, "undo_pos", "0")?;
    tx.commit().sql()?;
    conn.close().map_err(|(_, e)| e).sql()
}

/// 読み取りだけの接続も読み書きのフラグで開き、`query_only` で書き込みを止める。
/// 読み取りのフラグだと、WAL の `-shm` が無いときに開けないことがあるため。
fn open_connection(db: &Path, mode: OpenMode) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db, flags).sql()?;
    configure(&conn, mode)?;
    Ok(conn)
}

fn configure(conn: &Connection, mode: OpenMode) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))
        .sql()?;
    conn.pragma_update(None, "foreign_keys", true).sql()?;
    match mode {
        OpenMode::Writer => {
            conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
                .sql()?;
            conn.pragma_update(None, "synchronous", "NORMAL").sql()?;
        }
        OpenMode::ReadOnly => conn.pragma_update(None, "query_only", true).sql()?,
    }
    Ok(())
}

/// `PRAGMA quick_check`。通らなければ戻し先（バックアップと `mm import`）を添えて `Corrupt`。
fn check_integrity(conn: &Connection, dir: &Path) -> Result<()> {
    let result: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .sql()?;
    if result == "ok" {
        return Ok(());
    }
    let backups: Vec<String> = backup::list(dir)
        .iter()
        .map(|b| b.path.display().to_string())
        .collect();
    Err(MmError::Corrupt(format!(
        "project.mmdb が壊れている（{result}）。backups/ の新しいものか versions/ からの mm import --overwrite で戻す。バックアップ: {backups:?}"
    )))
}

fn require_current_schema(conn: &Connection) -> Result<()> {
    if schema::user_version(conn)? != schema::SCHEMA_VERSION {
        return Err(MmError::Unsupported(
            "古いバージョンの DB は書き手のプロセスだけが移行できる".into(),
        ));
    }
    Ok(())
}

fn migrate_with_backup(conn: &mut Connection, dir: &Path, now_ms: i64) -> Result<()> {
    if schema::needs_migration(conn, MIGRATIONS)? {
        backup::run(conn, dir, now_ms)?;
    }
    schema::migrate(conn, MIGRATIONS)
}

pub(crate) fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .sql()?;
    Ok(())
}

pub(crate) fn get_meta_i64(conn: &Connection, key: &str) -> Result<i64> {
    let v: String = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .sql()?;
    v.parse()
        .map_err(|_| MmError::Corrupt(format!("meta.{key} が数でない: {v}")))
}
