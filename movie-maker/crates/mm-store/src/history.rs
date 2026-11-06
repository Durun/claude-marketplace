//! 命令の記録の取り出し（`mm diff` と `mm log` の元）、名前付きの版、コメント、検索、`versions/` の書き出しと取り込み。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use mm_types::{MmError, Result};

use crate::export::{self, ExportReport, VERSIONS_DIR};
use crate::lock::WriterLock;
use crate::records::{kind_of, Actor, OpRecord, StoredOp, StoredTxn, TxnKind, VersionRow};
use crate::store::{SearchHit, Store, DB_FILE};
use crate::{backup, SqlExt};

/// FTS5 の trigram は 3 文字未満の語を引けないので、それ未満は部分一致で探す。
const TRIGRAM_MIN_CHARS: usize = 3;

fn op_from_row(r: &Row) -> rusqlite::Result<(i64, i64, String, String, String, String)> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
    ))
}

/// 取り消しと元のまとまりを対で消す。やり直しは直前の取り消しと対で消す。
/// 範囲に元のまとまりが無い取り消し（範囲の前に当てたものの取り消し）は、逆命令のまとまりとして残る。
pub fn fold_history(txns: Vec<StoredTxn>) -> Vec<StoredTxn> {
    let mut out: Vec<StoredTxn> = Vec::with_capacity(txns.len());
    for txn in txns {
        let cancels = |t: &StoredTxn| match txn.kind {
            TxnKind::Normal => false,
            TxnKind::Undo { of } => t.id as u64 == of || t.kind == TxnKind::Redo { of },
            TxnKind::Redo { of } => t.kind == TxnKind::Undo { of },
        };
        match out.iter().rposition(cancels) {
            Some(i) => {
                out.remove(i);
            }
            None => out.push(txn),
        }
    }
    out
}

impl Store {
    /// まとまりの命令を当てた順に返す。
    pub(crate) fn txn_ops(&self, txn_id: i64) -> Result<Vec<StoredOp>> {
        let mut st = self
            .conn
            .prepare_cached(
                "SELECT seq, doc_version, name, op, inverse, touches FROM ops
                 WHERE txn_id = ?1 ORDER BY seq",
            )
            .sql()?;
        let raw: Vec<_> = st
            .query_map([txn_id], op_from_row)
            .sql()?
            .collect::<rusqlite::Result<_>>()
            .sql()?;
        raw.into_iter()
            .map(|(seq, doc_version, name, op, inverse, touches)| {
                Ok(StoredOp {
                    seq,
                    doc_version,
                    name,
                    record: OpRecord {
                        op: serde_json::from_str(&op)?,
                        inverse: serde_json::from_str(&inverse)?,
                        touches: serde_json::from_str(&touches)?,
                    },
                })
            })
            .collect()
    }

    /// 版番号が `(from, to]` の間に当たったまとまりを、命令つきで当てた順に返す。
    /// 取り消しと元のまとまりは対で畳む（[`fold_history`]）。`mm diff` の元。
    pub fn txns_between(&self, from: i64, to: i64) -> Result<Vec<StoredTxn>> {
        let raw = self.load_txns(
            "WHERE doc_version > ?1 AND doc_version <= ?2",
            params![from, to],
        )?;
        Ok(fold_history(raw))
    }

    /// 取り消しとやり直しも含め、書いた順の全まとまり（`mm log`）。
    pub fn txn_log(&self) -> Result<Vec<StoredTxn>> {
        self.load_txns("", params![])
    }

    fn load_txns(&self, filter: &str, args: impl rusqlite::Params) -> Result<Vec<StoredTxn>> {
        let mut st = self
            .conn
            .prepare(&format!(
                "SELECT id, kind, of_txn, name, actor_kind, actor_name, at_ms, responds_to,
                        base_version, doc_version, first_seq, last_seq
                 FROM txns {filter} ORDER BY id"
            ))
            .sql()?;
        let heads: Vec<StoredTxn> = st
            .query_map(args, |r| {
                let kind: String = r.get(1)?;
                Ok(StoredTxn {
                    id: r.get(0)?,
                    kind: kind_of(&kind, r.get(2)?),
                    name: r.get(3)?,
                    actor: Actor {
                        kind: r.get(4)?,
                        name: r.get(5)?,
                    },
                    at_ms: r.get(6)?,
                    responds_to: r.get(7)?,
                    base_version: r.get(8)?,
                    doc_version: r.get(9)?,
                    first_seq: r.get(10)?,
                    last_seq: r.get(11)?,
                    ops: Vec::new(),
                })
            })
            .sql()?
            .collect::<rusqlite::Result<_>>()
            .sql()?;
        heads
            .into_iter()
            .map(|mut t| {
                t.ops = self.txn_ops(t.id)?;
                Ok(t)
            })
            .collect()
    }

    /// `base` より後に当たった命令（取り消しとやり直しも）の (seq, touches)。古い版を元にした命令の競合の判定に使う。
    pub fn touches_since(&self, base: i64) -> Result<Vec<(i64, Vec<String>)>> {
        let mut st = self
            .conn
            .prepare("SELECT seq, touches FROM ops WHERE doc_version > ?1 ORDER BY seq")
            .sql()?;
        let raw: Vec<(i64, String)> = st
            .query_map([base], |r| Ok((r.get(0)?, r.get(1)?)))
            .sql()?
            .collect::<rusqlite::Result<_>>()
            .sql()?;
        raw.into_iter()
            .map(|(seq, t)| Ok((seq, serde_json::from_str(&t)?)))
            .collect()
    }

    /// 時刻（UNIX のミリ秒）までに書かれた最後の版番号（取り消しとやり直しも数える）。`@{時刻}` の指定を版番号にする。
    pub fn doc_version_at(&self, at_ms: i64) -> Result<i64> {
        let v: Option<i64> = self
            .conn
            .query_row(
                "SELECT max(doc_version) FROM txns WHERE at_ms <= ?1",
                [at_ms],
                |r| r.get(0),
            )
            .sql()?;
        Ok(v.unwrap_or(0))
    }

    /// 名前付きの版。
    pub fn version_by_name(&self, name: &str) -> Result<Option<VersionRow>> {
        self.conn
            .query_row(
                "SELECT name, at_ms, actor, doc_version, ops_seq, hash, git_commit
                 FROM versions WHERE name = ?1",
                [name],
                |r| {
                    Ok(VersionRow {
                        name: r.get(0)?,
                        at_ms: r.get(1)?,
                        actor: r.get(2)?,
                        doc_version: r.get(3)?,
                        ops_seq: r.get(4)?,
                        hash: r.get(5)?,
                        git_commit: r.get(6)?,
                    })
                },
            )
            .optional()
            .sql()
    }

    fn insert_version(&self, v: &VersionRow) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO versions(name, at_ms, actor, doc_version, ops_seq, hash, git_commit)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    v.name,
                    v.at_ms,
                    v.actor,
                    v.doc_version,
                    v.ops_seq,
                    v.hash,
                    v.git_commit
                ],
            )
            .sql()?;
        Ok(())
    }

    fn last_seq(&self) -> Result<i64> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT max(seq) FROM ops", [], |r| r.get(0))
            .sql()?;
        Ok(v.unwrap_or(0))
    }

    /// コメントを足すか置き換える。`id` と `state` が要り、`comp_id` があれば列に抜く。
    pub fn put_comment(&self, comment: &Value) -> Result<()> {
        self.require_writer()?;
        let id = comment["id"]
            .as_str()
            .ok_or_else(|| MmError::InvalidArgument("コメントに id が無い".into()))?;
        let state = comment["state"]
            .as_str()
            .ok_or_else(|| MmError::InvalidArgument("コメントに state が無い".into()))?;
        self.conn
            .execute(
                "INSERT INTO comments(id, comp_id, state, json) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET comp_id = excluded.comp_id,
                   state = excluded.state, json = excluded.json",
                params![id, comment["comp_id"].as_str(), state, comment.to_string()],
            )
            .sql()?;
        Ok(())
    }

    /// コメントを ID の順に返す。
    pub fn comments(&self) -> Result<Vec<Value>> {
        let mut st = self
            .conn
            .prepare("SELECT json FROM comments ORDER BY id")
            .sql()?;
        let texts: Vec<String> = st
            .query_map([], |r| r.get(0))
            .sql()?
            .collect::<rusqlite::Result<_>>()
            .sql()?;
        texts
            .iter()
            .map(|t| serde_json::from_str(t).map_err(MmError::from))
            .collect()
    }

    /// レイヤー名、マーカー名、コンポジション、素材のパスを引く。3 文字以上は FTS5、未満は部分一致。
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let (sql, arg) = if query.chars().count() >= TRIGRAM_MIN_CHARS {
            (
                "SELECT kind, owner, text FROM search WHERE search MATCH ?1 ORDER BY rank LIMIT ?2",
                format!("\"{}\"", query.replace('"', "\"\"")),
            )
        } else {
            (
                "SELECT kind, owner, text FROM search WHERE text LIKE ?1 ESCAPE '\\' LIMIT ?2",
                format!(
                    "%{}%",
                    query
                        .replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                ),
            )
        };
        let mut st = self.conn.prepare(sql).sql()?;
        let hits = st
            .query_map(params![arg, limit as i64], |r| {
                Ok(SearchHit {
                    kind: r.get(0)?,
                    owner: r.get(1)?,
                    text: r.get(2)?,
                })
            })
            .sql()?
            .collect::<rusqlite::Result<_>>()
            .sql()?;
        Ok(hits)
    }

    fn export_files(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        export::render_files(&self.project, &self.comments()?)
    }

    /// `versions/` に決まったパスで上書きする。中身の変わったファイルだけ書く。
    pub fn export_versions(&self) -> Result<ExportReport> {
        let files = self.export_files()?;
        let written = export::write_files(&self.dir.join(VERSIONS_DIR), &files)?;
        Ok(ExportReport {
            hash: export::hash_files(&files),
            files: files.keys().cloned().collect(),
            written,
        })
    }

    /// 書き出して、名前付きの版として `versions` の表に残す（`mm export --name`）。
    /// `git tag` は CLI が打ち、コミットが分かれば `git_commit` に渡す。
    pub fn export_named_version(
        &self,
        name: &str,
        actor: &str,
        at_ms: i64,
        git_commit: Option<String>,
    ) -> Result<ExportReport> {
        self.require_writer()?;
        let report = self.export_versions()?;
        self.insert_version(&VersionRow {
            name: name.to_string(),
            at_ms,
            actor: actor.to_string(),
            doc_version: self.doc_version,
            ops_seq: self.last_seq()?,
            hash: report.hash.clone(),
            git_commit,
        })?;
        Ok(report)
    }

    /// `versions/` から DB を作り直す。文書とコメントが入り、`ops` は空から始まり、
    /// `versions` に import の行が付く。DB がすでにあれば `overwrite` のときだけ、写しを `backups/` に残して置き換える。
    pub fn import_versions(
        project_dir: &Path,
        versions_dir: &Path,
        overwrite: bool,
        now_ms: i64,
    ) -> Result<Store> {
        let files_hash;
        let (project, comments) = {
            let (p, c) = export::read_files(versions_dir)?;
            files_hash = export::hash_files(&export::render_files(&p, &c)?);
            (p, c)
        };
        if project_dir.join(DB_FILE).exists() {
            if !overwrite {
                return Err(MmError::InvalidArgument(
                    "project.mmdb がある。置き換えるなら overwrite".into(),
                ));
            }
            replace_existing(project_dir, now_ms)?;
        }
        let store = Store::create(project_dir, &project, now_ms)?;
        for c in &comments {
            store.put_comment(c)?;
        }
        store.insert_version(&VersionRow {
            name: format!("import-{}", backup::format_timestamp(now_ms)),
            at_ms: now_ms,
            actor: "import".into(),
            doc_version: 0,
            ops_seq: 0,
            hash: files_hash,
            git_commit: None,
        })?;
        Ok(store)
    }
}

/// 既存の DB の写しを `backups/` に残し、DB のファイルを消す。書き手のロックを取って他の書き手を締め出す。
/// 壊れていて写しを作れないときは、生のファイル（`-wal`、`-shm` も）をそのまま `backups/` に移す。
fn replace_existing(dir: &Path, now_ms: i64) -> Result<()> {
    let lock = WriterLock::try_acquire(dir)?
        .ok_or_else(|| MmError::InvalidArgument("別のプロセスが書き手".into()))?;
    let copied = Store::open_for_backup(dir).and_then(|old| backup::run(&old, dir, now_ms));
    match copied {
        Ok(_) => {
            for suffix in ["", "-wal", "-shm"] {
                let _ = fs::remove_file(dir.join(format!("{DB_FILE}{suffix}")));
            }
        }
        Err(_) => backup::move_broken(dir, now_ms)?,
    }
    drop(lock);
    Ok(())
}

impl Store {
    /// 壊れていても写しだけは取れるように、整合の検査と移行をしないで接続する。
    fn open_for_backup(dir: &Path) -> Result<rusqlite::Connection> {
        rusqlite::Connection::open_with_flags(
            dir.join(DB_FILE),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .sql()
    }
}
