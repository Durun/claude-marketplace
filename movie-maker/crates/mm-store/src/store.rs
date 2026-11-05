//! `Store`: プロジェクトの DB を開き、文書と命令の記録を 1 つのトランザクションで書く。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use mm_doc::Project;
use mm_types::{MmError, Result};

use crate::docio::{self, RowMap};
use crate::lock::{self, LockInfo, WriterLock};
use crate::records::{CommitInfo, OpRecord, TxnInput, TxnState, UndoPlan};
use crate::schema::{self, MIGRATIONS};
use crate::sync::{detect_sync_folder, SyncFolder};
use crate::{backup, SqlExt};

/// DB のファイル名（プロジェクトの下）。
pub(crate) const DB_FILE: &str = "project.mmdb";
const BACKUP_INTERVAL_MS: i64 = 15 * 60_000;
/// 同期のフォルダーでは短くする。
const SYNC_BACKUP_INTERVAL_MS: i64 = 5 * 60_000;
const BUSY_TIMEOUT_MS: u64 = 5000;

/// 開き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// 書き手。プロジェクトに 1 プロセスだけ。
    Writer,
    /// 読み取りだけ。別のプロセスが書き手のとき、こうなる。
    ReadOnly,
}

/// 開いたときの注意。開くのは断らない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreWarning {
    /// 同期のフォルダーかネットワークの下にある。バックアップの間隔を縮める。
    SyncFolder(SyncFolder),
    /// 別のプロセスが書き手なので読み取りだけで開いた。
    WriterElsewhere(Option<LockInfo>),
}

/// 検索の 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// `layer`、`marker`、`composition`、`asset`。
    pub kind: String,
    /// 持ち主の行の鍵（`layers/<comp>/<id>` など）。
    pub owner: String,
    pub text: String,
}

/// 開いたプロジェクト。
#[derive(Debug)]
pub struct Store {
    pub(crate) dir: PathBuf,
    pub(crate) conn: Connection,
    pub(crate) mode: OpenMode,
    pub(crate) lock: Option<WriterLock>,
    pub(crate) project: Project,
    pub(crate) rows: RowMap,
    pub(crate) doc_version: i64,
    pub(crate) undo_pos: i64,
    pub(crate) warnings: Vec<StoreWarning>,
    pub(crate) last_backup_ms: i64,
    pub(crate) sync_folder: Option<SyncFolder>,
}

impl Store {
    /// 空の DB を作って文書を入れる。すでにあれば断る。書き手になれなければ断る。
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
        let mut conn = Connection::open(dir.join(DB_FILE)).sql()?;
        configure(&conn, OpenMode::Writer)?;
        schema::migrate(&mut conn, MIGRATIONS)?;
        let mut project = project.clone();
        project.normalize();
        let rows = docio::rows_of(&project)?;
        let tx = conn.transaction().sql()?;
        docio::write_all(&tx, &rows)?;
        set_meta(
            &tx,
            "project_id",
            &format!("{now_ms:x}-{}", std::process::id()),
        )?;
        set_meta(&tx, "created_at_ms", &now_ms.to_string())?;
        set_meta(&tx, "doc_version", "0")?;
        set_meta(&tx, "undo_pos", "0")?;
        tx.commit().sql()?;
        Ok(Self::assemble(
            dir,
            conn,
            OpenMode::Writer,
            Some(lock),
            project,
            rows,
            now_ms,
        ))
    }

    /// 開く。別のプロセスが書き手なら読み取りだけで開く。壊れていれば `Corrupt`。
    pub fn open(dir: &Path, now_ms: i64) -> Result<Self> {
        let db = dir.join(DB_FILE);
        if !db.exists() {
            return Err(MmError::NotFound(db.display().to_string()));
        }
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
        } else if schema::user_version(&conn)? != schema::SCHEMA_VERSION {
            return Err(MmError::Unsupported(
                "古い版の DB は書き手のプロセスだけが移行できる".into(),
            ));
        }
        let project = docio::read_project(&conn)?;
        let rows = docio::rows_of(&project)?;
        let mut store = Self::assemble(dir, conn, mode, lock, project, rows, now_ms);
        store.finish_open(now_ms)?;
        Ok(store)
    }

    fn assemble(
        dir: &Path,
        conn: Connection,
        mode: OpenMode,
        lock: Option<WriterLock>,
        project: Project,
        rows: RowMap,
        now_ms: i64,
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
            last_backup_ms: now_ms,
            sync_folder,
        }
    }

    fn finish_open(&mut self, now_ms: i64) -> Result<()> {
        self.doc_version = get_meta_i64(&self.conn, "doc_version")?;
        self.undo_pos = get_meta_i64(&self.conn, "undo_pos")?;
        if self.mode == OpenMode::ReadOnly {
            let info = lock::read_info(&self.dir);
            self.warnings.push(StoreWarning::WriterElsewhere(info));
            return Ok(());
        }
        if docio::stored_doc_version(&self.conn)? < mm_doc::SCHEMA_VERSION {
            backup::run(&self.conn, &self.dir, now_ms)?;
            let tx = self.conn.transaction().sql()?;
            docio::write_all(&tx, &self.rows)?;
            tx.commit().sql()?;
        }
        Ok(())
    }

    /// 開き方。
    pub fn mode(&self) -> OpenMode {
        self.mode
    }

    /// 開いたときの注意。
    pub fn warnings(&self) -> &[StoreWarning] {
        &self.warnings
    }

    /// プロジェクトのフォルダー。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 今の文書。
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// 今の文書の版番号。命令を 1 つ当てるたび、取り消しとやり直しのたびに進む。
    pub fn doc_version(&self) -> i64 {
        self.doc_version
    }

    /// 取り消せるトランザクションの数。
    pub fn undo_depth(&self) -> i64 {
        self.undo_pos
    }

    /// アプリが HTTP のポートを開いたら、`mm` が見つけられるように書く。
    pub fn publish_http_port(&self, port: u16) -> Result<()> {
        let lock = self.lock.as_ref().ok_or_else(not_writer)?;
        lock.write_info(&LockInfo {
            pid: std::process::id(),
            http_port: Some(port),
        })
    }

    /// 動作中の書き手の情報（プロジェクトのフォルダーから）。
    pub fn read_writer_info(dir: &Path) -> Option<LockInfo> {
        lock::read_info(dir)
    }

    pub(crate) fn require_writer(&self) -> Result<()> {
        if self.mode == OpenMode::Writer {
            Ok(())
        } else {
            Err(not_writer())
        }
    }

    /// 文書の変更と命令の記録を 1 つのトランザクションで書く。`project` は命令を当てた後の文書。
    /// 途中で落ちたら、開き直すと書く前の版に戻る。
    pub fn commit(&mut self, project: &Project, input: &TxnInput) -> Result<CommitInfo> {
        self.require_writer()?;
        if input.ops.is_empty() {
            return Err(MmError::InvalidArgument(
                "命令の無いトランザクション".into(),
            ));
        }
        let rows = self.next_rows(project)?;
        let tx = self.conn.transaction().sql()?;
        docio::apply_diff(&tx, &self.rows, &rows)?;
        insert_txn(&tx, input)?;
        let (first, last, version) = insert_ops(&tx, input, self.doc_version)?;
        tx.execute(
            "UPDATE txns SET first_seq = ?2, last_seq = ?3, doc_version = ?4 WHERE id = ?1",
            params![input.id, first, last, version],
        )
        .sql()?;
        tx.execute("DELETE FROM undo_stack WHERE pos > ?1", [self.undo_pos])
            .sql()?;
        let pos = self.undo_pos + 1;
        tx.execute(
            "INSERT INTO undo_stack(pos, txn_id) VALUES (?1, ?2)",
            params![pos, input.id],
        )
        .sql()?;
        set_meta(&tx, "doc_version", &version.to_string())?;
        set_meta(&tx, "undo_pos", &pos.to_string())?;
        tx.commit().sql()?;
        self.adopt(project, rows, version, pos);
        Ok(CommitInfo {
            first_seq: first,
            last_seq: last,
            doc_version: version,
        })
    }

    /// 今の文書との差だけを直列化して、新しい文書の行を作る。
    fn next_rows(&self, project: &Project) -> Result<RowMap> {
        let prev = docio::Prev {
            project: &self.project,
            rows: &self.rows,
        };
        docio::rows_after(Some(&prev), project)
    }

    fn adopt(&mut self, project: &Project, rows: RowMap, version: i64, pos: i64) {
        let mut p = project.clone();
        p.normalize();
        self.project = p;
        self.rows = rows;
        self.doc_version = version;
        self.undo_pos = pos;
    }

    /// 取り消すトランザクション。無ければ `None`。呼ぶ側が逆命令を逆順に当てて、[`Store::commit_undo`] で書く。
    pub fn undo_plan(&self) -> Result<Option<UndoPlan>> {
        if self.undo_pos == 0 {
            return Ok(None);
        }
        self.plan_at(self.undo_pos)
    }

    /// やり直すトランザクション。無ければ `None`。命令を当てた順に当てて、[`Store::commit_redo`] で書く。
    pub fn redo_plan(&self) -> Result<Option<UndoPlan>> {
        self.plan_at(self.undo_pos + 1)
    }

    fn plan_at(&self, pos: i64) -> Result<Option<UndoPlan>> {
        let found: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT t.id, t.name FROM undo_stack u JOIN txns t ON t.id = u.txn_id WHERE u.pos = ?1",
                [pos],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .sql()?;
        let Some((txn_id, name)) = found else {
            return Ok(None);
        };
        let ops = self
            .txn_ops(&txn_id)?
            .into_iter()
            .map(|o| o.record)
            .collect::<Vec<OpRecord>>();
        Ok(Some(UndoPlan { txn_id, name, ops }))
    }

    /// 取り消した後の文書を書く。位置が 1 戻り、トランザクションは `undone` になる。
    pub fn commit_undo(&mut self, project: &Project) -> Result<i64> {
        self.finish_history(project, self.undo_pos, -1, TxnState::Undone)
    }

    /// やり直した後の文書を書く。位置が 1 進み、トランザクションは `applied` に戻る。
    pub fn commit_redo(&mut self, project: &Project) -> Result<i64> {
        self.finish_history(project, self.undo_pos + 1, 1, TxnState::Applied)
    }

    fn finish_history(
        &mut self,
        project: &Project,
        txn_pos: i64,
        delta: i64,
        state: TxnState,
    ) -> Result<i64> {
        self.require_writer()?;
        let rows = self.next_rows(project)?;
        let version = self.doc_version + 1;
        let pos = self.undo_pos + delta;
        let tx = self.conn.transaction().sql()?;
        docio::apply_diff(&tx, &self.rows, &rows)?;
        let changed = tx
            .execute(
                "UPDATE txns SET state = ?1, doc_version = ?2
                 WHERE id = (SELECT txn_id FROM undo_stack WHERE pos = ?3)",
                params![state.as_str(), version, txn_pos],
            )
            .sql()?;
        if changed != 1 {
            return Err(MmError::NotFound(format!("undo_stack の位置 {txn_pos}")));
        }
        set_meta(&tx, "doc_version", &version.to_string())?;
        set_meta(&tx, "undo_pos", &pos.to_string())?;
        tx.commit().sql()?;
        self.adopt(project, rows, version, pos);
        Ok(version)
    }

    /// 一貫した写しを `backups/` に作り、世代を整理する。
    pub fn backup(&mut self, now_ms: i64) -> Result<PathBuf> {
        let path = backup::run(&self.conn, &self.dir, now_ms)?;
        self.last_backup_ms = now_ms;
        Ok(path)
    }

    /// 間隔（15 分。同期のフォルダーは 5 分）が過ぎていたらバックアップを作る。編集中に定期的に呼ぶ。
    pub fn backup_if_due(&mut self, now_ms: i64) -> Result<Option<PathBuf>> {
        if self.mode != OpenMode::Writer || now_ms - self.last_backup_ms < self.backup_interval() {
            return Ok(None);
        }
        self.backup(now_ms).map(Some)
    }

    /// バックアップの間隔（ミリ秒）。
    pub fn backup_interval(&self) -> i64 {
        if self.sync_folder.is_some() {
            SYNC_BACKUP_INTERVAL_MS
        } else {
            BACKUP_INTERVAL_MS
        }
    }

    /// WAL を本体に戻す。
    pub fn checkpoint(&self) -> Result<()> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .sql()
    }

    /// 閉じる。バックアップと WAL の書き戻しをして、同期のフォルダーなら `versions/` も書き出す。
    pub fn close(mut self, now_ms: i64) -> Result<()> {
        if self.mode == OpenMode::Writer {
            self.backup(now_ms)?;
            if self.sync_folder.is_some() {
                self.export_versions()?;
            }
            self.checkpoint()?;
        }
        Ok(())
    }
}

fn not_writer() -> MmError {
    MmError::Unsupported("読み取りだけで開いているので書けない".into())
}

fn open_connection(db: &Path, mode: OpenMode) -> Result<Connection> {
    let flags = match mode {
        OpenMode::Writer => OpenFlags::SQLITE_OPEN_READ_WRITE,
        OpenMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db, flags).sql()?;
    configure(&conn, mode)?;
    Ok(conn)
}

fn configure(conn: &Connection, mode: OpenMode) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))
        .sql()?;
    conn.pragma_update(None, "foreign_keys", true).sql()?;
    if mode == OpenMode::Writer {
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
            .sql()?;
        conn.pragma_update(None, "synchronous", "NORMAL").sql()?;
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
        "project.mmdb が壊れている（{result}）。backups/ の新しいものか versions/ からの mm import で戻す。バックアップ: {backups:?}"
    )))
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

fn insert_txn(tx: &rusqlite::Transaction, input: &TxnInput) -> Result<()> {
    tx.execute(
        "INSERT INTO txns(id, first_seq, last_seq, name, state, actor_kind, actor_name, at_ms, doc_version)
         VALUES (?1, 0, 0, ?2, 'applied', ?3, ?4, ?5, 0)",
        params![input.id, input.name, input.actor.kind, input.actor.name, input.at_ms],
    )
    .sql()?;
    Ok(())
}

/// 命令を `ops` に書く。戻りは (最初の seq, 最後の seq, 最後の版番号)。命令ごとに版番号が 1 進む。
fn insert_ops(
    tx: &rusqlite::Transaction,
    input: &TxnInput,
    base_version: i64,
) -> Result<(i64, i64, i64)> {
    let mut version = base_version;
    let (mut first, mut last) = (0, 0);
    let mut st = tx
        .prepare_cached(
            "INSERT INTO ops(txn_id, actor_kind, actor_name, at_ms, name, op, inverse, touches, responds_to, doc_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .sql()?;
    for op in &input.ops {
        version += 1;
        st.execute(params![
            input.id,
            input.actor.kind,
            input.actor.name,
            input.at_ms,
            op_name(&op.op),
            op.op.to_string(),
            op.inverse.to_string(),
            serde_json::to_string(&op.touches)?,
            input.responds_to,
            version
        ])
        .sql()?;
        last = tx.last_insert_rowid();
        if first == 0 {
            first = last;
        }
    }
    Ok((first, last, version))
}

/// 命令の名前。`type` か `op` の文字があれば使う。
fn op_name(op: &serde_json::Value) -> String {
    ["type", "op"]
        .iter()
        .find_map(|k| op.get(k).and_then(|v| v.as_str()))
        .unwrap_or("?")
        .to_string()
}
