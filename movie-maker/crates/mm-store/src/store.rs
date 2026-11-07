//! `Store`: 開いたプロジェクト。プロジェクトのデータと操作の記録を 1 つのトランザクションで書く。
//! 開き方（作る、開く、読み取りだけ）は `open.rs`。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use mm_doc::Project;
use mm_types::{MmError, Result};

use crate::backup::BackupJob;
use crate::docio::{self, RowMap};
use crate::lock::{self, LockInfo, WriterLock};
use crate::open::set_meta;
use crate::records::{kind_str, Actor, OpRecord, StoredTxn, TxnInput, TxnKind, UndoPlan};
use crate::sync::SyncFolder;
use crate::{backup, SqlExt};

/// DB のファイル名（プロジェクトの下）。
pub(crate) const DB_FILE: &str = "project.mmdb";
const BACKUP_INTERVAL_MS: i64 = 15 * 60_000;
/// 同期のフォルダーでは短くする。
const SYNC_BACKUP_INTERVAL_MS: i64 = 5 * 60_000;

/// 開き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// 書き手。プロジェクトに 1 プロセスだけ。
    Writer,
    /// 読み取りだけ。別のプロセスが書き手のとき、または読むだけで開いたとき。
    ReadOnly,
}

/// 開いたときの注意。開くのは拒否しない。
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
    /// `layer`、`marker`、`composition`、`asset`、`topic`（話題の見出し）、`tag`（選別と話題のタグ）。
    pub kind: String,
    /// 持ち主の行のキー（`layers/<comp>/<id>` など）。
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
    /// 開き方。
    pub fn mode(&self) -> OpenMode {
        self.mode
    }

    /// 開き方が読み取りだけか。
    pub fn is_read_only(&self) -> bool {
        self.mode == OpenMode::ReadOnly
    }

    /// 開いたときの注意。
    pub fn warnings(&self) -> &[StoreWarning] {
        &self.warnings
    }

    /// プロジェクトのフォルダー。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 今のプロジェクトのデータ。
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// 今のプロジェクトのデータのバージョン番号。操作 1 つで 1 進む。取り消しとやり直しも操作の数だけ進める。
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
        lock.write_info(Some(port))
    }

    /// HTTP サーバーを閉じたら、ポートとトークンを消す。`mm` は書き手が HTTP を持たないものとして扱う。
    pub fn unpublish_http(&self) -> Result<()> {
        let lock = self.lock.as_ref().ok_or_else(not_writer)?;
        lock.clear_token();
        lock.write_info(None)
    }

    /// 起動ごとのトークンを、プロジェクトの外の持ち主専用のフォルダーに書く。
    /// HTTP のクライアントはこれを持っていなければ通らない。
    pub fn publish_token(&self, token: &str) -> Result<()> {
        self.lock
            .as_ref()
            .ok_or_else(not_writer)?
            .write_token(token)
    }

    /// 動作中の書き手のトークン（プロジェクトのフォルダーから）。
    pub fn read_writer_token(dir: &Path) -> Option<String> {
        lock::read_token(dir)
    }

    /// トークンのファイルのパス（権限の検査用）。
    pub fn writer_token_file(dir: &Path) -> Option<PathBuf> {
        lock::token_file(dir)
    }

    /// 書き手のプロセスが生きているか。`false` なら `lock.info` の中身は古い。
    pub fn writer_alive(dir: &Path) -> bool {
        lock::writer_alive(dir)
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

    /// プロジェクトのデータの変更とトランザクションの記録を 1 つのトランザクションで書く。`project` は操作を適用した後のプロジェクトのデータ。
    /// 取り消し（やり直し）は、逆操作（元の操作）を適用した記録を同じ形で書き、バージョンも操作の数だけ進む。
    /// 適用したトランザクションの行は後から書き換えない。途中でクラッシュしたら、開き直すと書く前のバージョンに戻る。
    pub fn commit(&mut self, project: &Project, input: &TxnInput) -> Result<StoredTxn> {
        self.commit_with(project, input, |_, _| Ok(()))
    }

    /// [`Store::commit`] と同じく書き、同じ SQLite のトランザクションの中で `extra` も書く。
    /// 提案の承認は、適用したトランザクションと提案の状態を一緒に書き、途中でクラッシュしても片方だけ残らないようにする。
    pub(crate) fn commit_with(
        &mut self,
        project: &Project,
        input: &TxnInput,
        extra: impl FnOnce(&rusqlite::Transaction, &StoredTxn) -> Result<()>,
    ) -> Result<StoredTxn> {
        self.require_writer()?;
        if input.ops.is_empty() {
            return Err(MmError::InvalidArgument(
                "操作の無いトランザクション".into(),
            ));
        }
        let pos = self.next_undo_pos(input.kind)?;
        let rows = self.next_rows(project)?;
        let tx = self.conn.transaction().sql()?;
        docio::apply_diff(&tx, &self.rows, &rows)?;
        crate::transcript::sync_edits(&tx, &self.project, project)?;
        let stored = insert_txn(&tx, input, self.doc_version)?;
        if input.kind == TxnKind::Normal {
            tx.execute("DELETE FROM undo_stack WHERE pos > ?1", [self.undo_pos])
                .sql()?;
            tx.execute(
                "INSERT INTO undo_stack(pos, txn_id) VALUES (?1, ?2)",
                params![pos, stored.id],
            )
            .sql()?;
        }
        set_meta(&tx, "doc_version", &stored.doc_version.to_string())?;
        set_meta(&tx, "undo_pos", &pos.to_string())?;
        extra(&tx, &stored)?;
        tx.commit().sql()?;
        self.adopt(project, rows, stored.doc_version, pos);
        Ok(stored)
    }

    /// 書いた後の取り消しの位置。取り消しとやり直しは、列の今の位置のトランザクションを指していなければ拒否する。
    fn next_undo_pos(&self, kind: TxnKind) -> Result<i64> {
        let expect = |pos: i64, of: u64| -> Result<()> {
            if self.stack_at(pos)? == Some(of as i64) {
                Ok(())
            } else {
                Err(MmError::InvalidArgument(format!(
                    "トランザクション {of} は取り消しの列の位置 {pos} に無い"
                )))
            }
        };
        match kind {
            TxnKind::Normal => Ok(self.undo_pos + 1),
            TxnKind::Undo { of } => {
                expect(self.undo_pos, of)?;
                Ok(self.undo_pos - 1)
            }
            TxnKind::Redo { of } => {
                expect(self.undo_pos + 1, of)?;
                Ok(self.undo_pos + 1)
            }
        }
    }

    fn stack_at(&self, pos: i64) -> Result<Option<i64>> {
        self.conn
            .query_row("SELECT txn_id FROM undo_stack WHERE pos = ?1", [pos], |r| {
                r.get(0)
            })
            .optional()
            .sql()
    }

    /// 今のプロジェクトのデータとの差だけを直列化して、新しいプロジェクトのデータの行を作る。
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

    /// 取り消すトランザクション。無ければ `None`。呼ぶ側が逆操作を逆順に適用し、`TxnKind::Undo` の [`Store::commit`] で書く。
    pub fn undo_plan(&self) -> Result<Option<UndoPlan>> {
        if self.undo_pos == 0 {
            return Ok(None);
        }
        self.plan_at(self.undo_pos)
    }

    /// やり直すトランザクション。無ければ `None`。操作を適用した順に適用し、`TxnKind::Redo` の [`Store::commit`] で書く。
    pub fn redo_plan(&self) -> Result<Option<UndoPlan>> {
        self.plan_at(self.undo_pos + 1)
    }

    fn plan_at(&self, pos: i64) -> Result<Option<UndoPlan>> {
        let found: Option<(i64, String, Actor)> = self
            .conn
            .query_row(
                "SELECT t.id, t.name, t.actor_kind, t.actor_name
                 FROM undo_stack u JOIN txns t ON t.id = u.txn_id WHERE u.pos = ?1",
                [pos],
                |r| {
                    let actor = Actor {
                        kind: r.get(2)?,
                        name: r.get(3)?,
                    };
                    Ok((r.get(0)?, r.get(1)?, actor))
                },
            )
            .optional()
            .sql()?;
        let Some((txn_id, name, actor)) = found else {
            return Ok(None);
        };
        let ops = self
            .txn_ops(txn_id)?
            .into_iter()
            .map(|o| o.record)
            .collect::<Vec<OpRecord>>();
        Ok(Some(UndoPlan {
            txn_id,
            name,
            actor,
            ops,
        }))
    }

    /// 一貫したコピーを `backups/` に作り、世代を整理する。
    pub fn backup(&mut self, now_ms: i64) -> Result<PathBuf> {
        let path = backup::run(&self.conn, &self.dir, now_ms)?;
        self.last_backup_ms = now_ms;
        Ok(path)
    }

    /// 間隔（15 分。同期のフォルダーは 5 分）が過ぎていたらバックアップを作る。
    /// 前回の時刻は `backups/` の新しいファイルから読むので、プロセスをまたいでも間隔が守られる。
    pub fn backup_if_due(&mut self, now_ms: i64) -> Result<Option<PathBuf>> {
        if self.mode != OpenMode::Writer || now_ms - self.last_backup_ms < self.backup_interval() {
            return Ok(None);
        }
        self.backup(now_ms).map(Some)
    }

    /// 間隔が過ぎていたら、バックアップの仕事を返す（時刻は今に進める）。
    /// 仕事は `Store` の外で走らせられるので、並べて使う側のロックを長く持たずに済む。
    pub fn take_due_backup(&mut self, now_ms: i64) -> Option<BackupJob> {
        if self.mode != OpenMode::Writer || now_ms - self.last_backup_ms < self.backup_interval() {
            return None;
        }
        self.last_backup_ms = now_ms;
        Some(BackupJob::new(self.dir.clone(), now_ms))
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
    /// アプリが使う（編集の区切り）。
    pub fn close(self, now_ms: i64) -> Result<()> {
        self.close_with(now_ms, true)
    }

    /// 閉じる。バックアップは間隔が過ぎていたときだけ作る。1 操作ごとに開き直す `mm` が使う
    /// （毎回コピーすると直近 10 個の世代が数分で埋まり、古い状態に戻れなくなる）。
    pub fn close_brief(self, now_ms: i64) -> Result<()> {
        self.close_with(now_ms, false)
    }

    fn close_with(mut self, now_ms: i64, force_backup: bool) -> Result<()> {
        if self.mode != OpenMode::Writer {
            return Ok(());
        }
        if force_backup {
            self.backup(now_ms)?;
        } else {
            self.backup_if_due(now_ms)?;
        }
        if self.sync_folder.is_some() {
            self.export_versions()?;
        }
        self.checkpoint()
    }
}

fn not_writer() -> MmError {
    MmError::Unsupported("読み取りだけで開いているので書けない".into())
}

/// トランザクションと操作を表に書く。ID は通し番号、バージョンは操作ごとに 1 進む。
fn insert_txn(tx: &rusqlite::Transaction, input: &TxnInput, base: i64) -> Result<StoredTxn> {
    let id: i64 = tx
        .query_row("SELECT coalesce(max(id), 0) + 1 FROM txns", [], |r| {
            r.get(0)
        })
        .sql()?;
    let version = base + input.ops.len() as i64;
    tx.execute(
        "INSERT INTO txns(id, kind, of_txn, name, actor_kind, actor_name, at_ms, responds_to,
                          base_version, doc_version, first_seq, last_seq)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 0)",
        params![
            id,
            kind_str(input.kind),
            input.kind.of().map(|o| o as i64),
            input.name,
            input.actor.kind,
            input.actor.name,
            input.at_ms,
            input.responds_to,
            base,
            version
        ],
    )
    .sql()?;
    let ops = insert_ops(tx, id, input, base)?;
    let (first_seq, last_seq) = (ops[0].seq, ops[ops.len() - 1].seq);
    tx.execute(
        "UPDATE txns SET first_seq = ?2, last_seq = ?3 WHERE id = ?1",
        params![id, first_seq, last_seq],
    )
    .sql()?;
    Ok(StoredTxn {
        id,
        kind: input.kind,
        name: input.name.clone(),
        actor: input.actor.clone(),
        at_ms: input.at_ms,
        responds_to: input.responds_to.clone(),
        base_version: base,
        doc_version: version,
        first_seq,
        last_seq,
        ops,
    })
}

fn insert_ops(
    tx: &rusqlite::Transaction,
    txn_id: i64,
    input: &TxnInput,
    base: i64,
) -> Result<Vec<crate::records::StoredOp>> {
    let mut st = tx
        .prepare_cached(
            "INSERT INTO ops(txn_id, name, op, inverse, touches, doc_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .sql()?;
    let mut out = Vec::with_capacity(input.ops.len());
    for (i, op) in input.ops.iter().enumerate() {
        let doc_version = base + i as i64 + 1;
        let name = op_name(&op.op);
        st.execute(params![
            txn_id,
            name,
            op.op.to_string(),
            op.inverse.to_string(),
            serde_json::to_string(&op.touches)?,
            doc_version
        ])
        .sql()?;
        out.push(crate::records::StoredOp {
            seq: tx.last_insert_rowid(),
            doc_version,
            name,
            record: op.clone(),
        });
    }
    Ok(out)
}

/// 操作の名前（`op` のフィールド）。
fn op_name(op: &serde_json::Value) -> String {
    op["op"].as_str().unwrap_or("?").to_string()
}
