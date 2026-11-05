//! 表の DDL と `user_version` による移行。

use rusqlite::Connection;

use mm_types::{MmError, Result};

use crate::SqlExt;

/// `PRAGMA application_id`。ASCII の "MMDB"。`.mmdb` の見分けに使う。
pub const APPLICATION_ID: i32 = 0x4d4d_4442;

/// DDL を 1 つずつ当てた先の版。`MIGRATIONS` の長さと一致する。
pub const SCHEMA_VERSION: u32 = 1;

/// 版 `i` から `i + 1` に上げる DDL。文書の JSON の形の移行は `mm_doc::migrate` が持つ。
pub(crate) const MIGRATIONS: &[&str] = &[V1];

const V1: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE project (id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL);
CREATE TABLE assets (
  id TEXT PRIMARY KEY, content_hash TEXT NOT NULL, path TEXT NOT NULL, json TEXT NOT NULL
) WITHOUT ROWID;
CREATE INDEX assets_content_hash ON assets(content_hash);
CREATE TABLE compositions (id TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE layers (
  comp_id TEXT NOT NULL REFERENCES compositions(id) ON DELETE CASCADE,
  id TEXT NOT NULL,
  z INTEGER NOT NULL,
  json TEXT NOT NULL,
  PRIMARY KEY (comp_id, id)
) WITHOUT ROWID;
CREATE INDEX layers_z ON layers(comp_id, z);
CREATE TABLE sequences (id TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE render_targets (id TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE txns (
  id TEXT PRIMARY KEY,
  first_seq INTEGER NOT NULL,
  last_seq INTEGER NOT NULL,
  name TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('applied', 'undone')),
  actor_kind TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  at_ms INTEGER NOT NULL,
  doc_version INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX txns_doc_version ON txns(doc_version);
CREATE TABLE ops (
  seq INTEGER PRIMARY KEY,
  txn_id TEXT NOT NULL REFERENCES txns(id),
  actor_kind TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  at_ms INTEGER NOT NULL,
  name TEXT NOT NULL,
  op TEXT NOT NULL,
  inverse TEXT NOT NULL,
  touches TEXT NOT NULL,
  responds_to TEXT,
  doc_version INTEGER NOT NULL
);
CREATE INDEX ops_txn ON ops(txn_id);
CREATE INDEX ops_doc_version ON ops(doc_version);
CREATE TABLE undo_stack (pos INTEGER PRIMARY KEY, txn_id TEXT NOT NULL REFERENCES txns(id));
CREATE TABLE versions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  at_ms INTEGER NOT NULL,
  actor TEXT NOT NULL,
  doc_version INTEGER NOT NULL,
  ops_seq INTEGER NOT NULL,
  hash TEXT NOT NULL,
  git_commit TEXT
);
CREATE TABLE comments (
  id TEXT PRIMARY KEY, comp_id TEXT, state TEXT NOT NULL, json TEXT NOT NULL
) WITHOUT ROWID;
CREATE VIRTUAL TABLE search USING fts5(
  text, kind UNINDEXED, owner UNINDEXED, tokenize = 'trigram'
);
";

/// DB の `user_version`。
pub(crate) fn user_version(conn: &Connection) -> Result<u32> {
    let v: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .sql()?;
    Ok(v)
}

/// 移行が要るか。新しい版は `Unsupported`。0（空の DB）は初期化として扱い、要るとは言わない。
pub(crate) fn needs_migration(conn: &Connection, steps: &[&str]) -> Result<bool> {
    let v = user_version(conn)?;
    check_not_newer(v, steps)?;
    Ok(v > 0 && (v as usize) < steps.len())
}

fn check_not_newer(v: u32, steps: &[&str]) -> Result<()> {
    if v as usize > steps.len() {
        return Err(MmError::Unsupported(format!(
            "DB のスキーマの版 {v} はこのビルド（{}）より新しい",
            steps.len()
        )));
    }
    Ok(())
}

/// 版を 1 つずつ最後まで上げる。全体を 1 つのトランザクションで囲み、失敗したら戻る。
pub(crate) fn migrate(conn: &mut Connection, steps: &[&str]) -> Result<()> {
    let current = user_version(conn)?;
    check_not_newer(current, steps)?;
    let tx = conn.transaction().sql()?;
    for (i, ddl) in steps.iter().enumerate().skip(current as usize) {
        tx.execute_batch(ddl).sql()?;
        tx.pragma_update(None, "user_version", i as u32 + 1).sql()?;
    }
    tx.pragma_update(None, "application_id", APPLICATION_ID)
        .sql()?;
    tx.commit().sql()
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2_TEST: &str = "CREATE TABLE extra (x INTEGER);";

    fn extra_tables(conn: &Connection) -> Vec<String> {
        let mut st = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='extra'")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn 古い版から_1_つずつ今の版まで上がる() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1]).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 1);
        assert!(needs_migration(&conn, &[V1, V2_TEST]).unwrap());
        migrate(&mut conn, &[V1, V2_TEST]).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 2);
        assert_eq!(extra_tables(&conn), ["extra"]);
    }

    #[test]
    fn 失敗した移行は戻る() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1]).unwrap();
        let bad = "CREATE TABLE half (x); THIS IS NOT SQL;";
        assert!(migrate(&mut conn, &[V1, bad]).is_err());
        assert_eq!(user_version(&conn).unwrap(), 1);
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='half'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn 新しい版の_db_は断る() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1, V2_TEST]).unwrap();
        assert!(matches!(
            migrate(&mut conn, &[V1]),
            Err(MmError::Unsupported(_))
        ));
    }
}
