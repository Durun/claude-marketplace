//! 表の DDL と `user_version` による移行。

use rusqlite::{Connection, OptionalExtension};

use mm_types::{MmError, Result};

use crate::SqlExt;

/// `PRAGMA application_id`。ASCII の "MMDB"。`.mmdb` の見分けに使う。
pub const APPLICATION_ID: i32 = 0x4d4d_4442;

/// DDL を 1 つずつ適用した先のバージョン。`MIGRATIONS` の長さと一致する。
pub const SCHEMA_VERSION: u32 = 4;

/// バージョン `i` から `i + 1` に上げる DDL。プロジェクトのデータの JSON の形の移行は `mm_doc::migrate` が持つ。
pub(crate) const MIGRATIONS: &[&str] = &[V1, V2, V3, V4];

/// バージョン 4: Claude の編集の提案（保留中のトランザクション）。承認で適用したトランザクションを `txn_id` で指す。
/// `ops` は提案した操作（`Op` の JSON の配列）、`touches` は受け付けたときに適用して求めた所、`approved_ops` は承認した操作の番号。
const V4: &str = "
CREATE TABLE proposals (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  entry TEXT NOT NULL,
  actor_kind TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  at_ms INTEGER NOT NULL,
  responds_to TEXT,
  base_version INTEGER NOT NULL,
  ops TEXT NOT NULL,
  touches TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'rejected')),
  decided_kind TEXT,
  decided_name TEXT,
  decided_at_ms INTEGER,
  txn_id INTEGER REFERENCES txns(id),
  approved_ops TEXT
);
CREATE INDEX proposals_state ON proposals(state);
";

/// バージョン 3: 素材の整理と文字起こしの修正の行（プロジェクトのデータ v6）と、文字起こしの本文と全文検索。
///
/// `words.run` は単語の文字に後ろの単語の文字を続けたもの（`transcript::RUN_EXTRA_CHARS` 文字まで）で、
/// 単語の境界をまたぐ語（「編」「集」の「編集」）も単語の行の検索で引けるようにする。
/// `words_fts` は `words` を外部コンテンツにした FTS5 で、`run` だけを索引にし、トリガーで同期する。
const V3: &str = "
CREATE TABLE marks (id TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE selections (id TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE topics (id TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE word_edits (id TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE transcripts (
  asset_id TEXT PRIMARY KEY,
  content_hash TEXT NOT NULL,
  producer TEXT NOT NULL,
  language TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE words (
  id INTEGER PRIMARY KEY,
  asset_id TEXT NOT NULL REFERENCES transcripts(asset_id),
  word_id TEXT NOT NULL,
  ord INTEGER NOT NULL,
  segment INTEGER NOT NULL,
  start_num INTEGER NOT NULL,
  start_den INTEGER NOT NULL,
  end_num INTEGER NOT NULL,
  end_den INTEGER NOT NULL,
  text TEXT NOT NULL,
  fixed TEXT,
  deleted INTEGER NOT NULL DEFAULT 0,
  run TEXT NOT NULL,
  UNIQUE (asset_id, word_id),
  UNIQUE (asset_id, ord)
);
CREATE VIRTUAL TABLE words_fts USING fts5(
  run, content = 'words', content_rowid = 'id', tokenize = 'trigram case_sensitive 1'
);
CREATE TRIGGER words_ai AFTER INSERT ON words BEGIN
  INSERT INTO words_fts(rowid, run) VALUES (new.id, new.run);
END;
CREATE TRIGGER words_ad AFTER DELETE ON words BEGIN
  INSERT INTO words_fts(words_fts, rowid, run) VALUES ('delete', old.id, old.run);
END;
CREATE TRIGGER words_au AFTER UPDATE OF run ON words BEGIN
  INSERT INTO words_fts(words_fts, rowid, run) VALUES ('delete', old.id, old.run);
  INSERT INTO words_fts(rowid, run) VALUES (new.id, new.run);
END;
";

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

/// バージョン 2: トランザクションの ID を整数にし、種類（`normal` / `undo` / `redo`）と元のトランザクション、適用する前のバージョンを持つ。
/// 適用した後のバージョンは書き換えないので `state` は要らない。`txns` と `ops` と `undo_stack` を作り直す。
const V2: &str = "
CREATE TABLE txn_ids (old TEXT PRIMARY KEY, new INTEGER NOT NULL) WITHOUT ROWID;
INSERT INTO txn_ids(old, new)
  SELECT id, row_number() OVER (ORDER BY first_seq) FROM txns;
CREATE TABLE txns_v2 (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('normal', 'undo', 'redo')),
  of_txn INTEGER REFERENCES txns_v2(id),
  name TEXT NOT NULL,
  actor_kind TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  at_ms INTEGER NOT NULL,
  responds_to TEXT,
  base_version INTEGER NOT NULL,
  doc_version INTEGER NOT NULL,
  first_seq INTEGER NOT NULL,
  last_seq INTEGER NOT NULL
);
INSERT INTO txns_v2(id, kind, of_txn, name, actor_kind, actor_name, at_ms, responds_to,
                    base_version, doc_version, first_seq, last_seq)
  SELECT m.new, 'normal', NULL, t.name, t.actor_kind, t.actor_name, t.at_ms,
         (SELECT responds_to FROM ops WHERE ops.txn_id = t.id ORDER BY seq LIMIT 1),
         (SELECT min(doc_version) - 1 FROM ops WHERE ops.txn_id = t.id),
         (SELECT max(doc_version) FROM ops WHERE ops.txn_id = t.id),
         t.first_seq, t.last_seq
  FROM txns t JOIN txn_ids m ON m.old = t.id;
CREATE TABLE ops_v2 (
  seq INTEGER PRIMARY KEY,
  txn_id INTEGER NOT NULL REFERENCES txns_v2(id),
  name TEXT NOT NULL,
  op TEXT NOT NULL,
  inverse TEXT NOT NULL,
  touches TEXT NOT NULL,
  doc_version INTEGER NOT NULL
);
INSERT INTO ops_v2(seq, txn_id, name, op, inverse, touches, doc_version)
  SELECT o.seq, m.new, o.name, o.op, o.inverse, o.touches, o.doc_version
  FROM ops o JOIN txn_ids m ON m.old = o.txn_id;
CREATE TABLE undo_stack_v2 (pos INTEGER PRIMARY KEY, txn_id INTEGER NOT NULL REFERENCES txns_v2(id));
INSERT INTO undo_stack_v2(pos, txn_id)
  SELECT u.pos, m.new FROM undo_stack u JOIN txn_ids m ON m.old = u.txn_id;
DROP TABLE undo_stack;
DROP TABLE ops;
DROP TABLE txns;
DROP TABLE txn_ids;
ALTER TABLE txns_v2 RENAME TO txns;
ALTER TABLE ops_v2 RENAME TO ops;
ALTER TABLE undo_stack_v2 RENAME TO undo_stack;
CREATE INDEX txns_doc_version ON txns(doc_version);
CREATE INDEX ops_txn ON ops(txn_id);
CREATE INDEX ops_doc_version ON ops(doc_version);
";

/// DB の `user_version`。
pub(crate) fn user_version(conn: &Connection) -> Result<u32> {
    let v: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .sql()?;
    Ok(v)
}

/// 移行が要るか。新しいバージョンは `Unsupported`。0（空の DB）は初期化として扱い、要るとは言わない。
pub(crate) fn needs_migration(conn: &Connection, steps: &[&str]) -> Result<bool> {
    let v = user_version(conn)?;
    check_not_newer(v, steps)?;
    Ok(v > 0 && (v as usize) < steps.len())
}

fn check_not_newer(v: u32, steps: &[&str]) -> Result<()> {
    if v as usize > steps.len() {
        return Err(MmError::Unsupported(format!(
            "DB のスキーマのバージョン {v} はこのビルド（{}）より新しい",
            steps.len()
        )));
    }
    Ok(())
}

/// バージョンを 1 つずつ最後まで上げる。全体を 1 つのトランザクションで囲み、失敗したら戻る。
/// 表の作り直しで親の表を削除するため、外部キーの検査は止めて、終わりに `foreign_key_check` で確かめる。
pub(crate) fn migrate(conn: &mut Connection, steps: &[&str]) -> Result<()> {
    let current = user_version(conn)?;
    check_not_newer(current, steps)?;
    conn.pragma_update(None, "foreign_keys", false).sql()?;
    let result = migrate_in_tx(conn, steps, current as usize);
    conn.pragma_update(None, "foreign_keys", true).sql()?;
    result
}

fn migrate_in_tx(conn: &mut Connection, steps: &[&str], from: usize) -> Result<()> {
    let tx = conn.transaction().sql()?;
    for (i, ddl) in steps.iter().enumerate().skip(from) {
        tx.execute_batch(ddl).sql()?;
        tx.pragma_update(None, "user_version", i as u32 + 1).sql()?;
    }
    tx.pragma_update(None, "application_id", APPLICATION_ID)
        .sql()?;
    let broken: Option<String> = tx
        .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
        .optional()
        .sql()?;
    if let Some(table) = broken {
        return Err(MmError::Corrupt(format!(
            "移行の後に外部キーが壊れている: {table}"
        )));
    }
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
    fn 古いバージョンから_1_つずつ今のバージョンまで上がる() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1]).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 1);
        assert!(needs_migration(&conn, &[V1, V2_TEST]).unwrap());
        migrate(&mut conn, &[V1, V2_TEST]).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 2);
        assert!(!needs_migration(&conn, &[V1, V2_TEST]).unwrap());
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
    fn バージョン_1_のトランザクションと操作は整数の_id_に写り_undo_stack_が追う() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1]).unwrap();
        conn.execute_batch(
            "INSERT INTO txns VALUES ('b', 3, 3, 'B', 'applied', 'cli', 'mm', 20, 3);
             INSERT INTO txns VALUES ('a', 1, 2, 'A', 'applied', 'gui', 'app', 10, 2);
             INSERT INTO ops(txn_id, actor_kind, actor_name, at_ms, name, op, inverse, touches, responds_to, doc_version)
               VALUES ('a', 'gui', 'app', 10, 'x', '{}', '{}', '[]', 'r1', 1),
                      ('a', 'gui', 'app', 10, 'x', '{}', '{}', '[]', 'r1', 2),
                      ('b', 'cli', 'mm', 20, 'y', '{}', '{}', '[]', NULL, 3);
             INSERT INTO undo_stack VALUES (1, 'a'), (2, 'b');",
        )
        .unwrap();
        migrate(&mut conn, &[V1, V2]).unwrap();
        let rows: Vec<(i64, String, i64, i64, Option<String>)> = conn
            .prepare(
                "SELECT id, kind, base_version, doc_version, responds_to FROM txns ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            [
                (1, "normal".into(), 0, 2, Some("r1".into())),
                (2, "normal".into(), 2, 3, None)
            ]
        );
        let stack: Vec<(i64, i64)> = conn
            .prepare("SELECT pos, txn_id FROM undo_stack ORDER BY pos")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(stack, [(1, 1), (2, 2)]);
        let ops_of_b: i64 = conn
            .query_row("SELECT count(*) FROM ops WHERE txn_id = 2", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(ops_of_b, 1);
    }

    #[test]
    fn バージョン_2_の_db_に整理と文字起こしの表が足される() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1, V2]).unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
        conn.execute_batch(
            "INSERT INTO transcripts VALUES ('a', 'h', 'p', 'ja');
             INSERT INTO words(asset_id, word_id, ord, segment, start_num, start_den, end_num, end_den, text, run)
               VALUES ('a', 'w', 0, 0, 0, 1, 1, 1, '動画編集', '動画編集');",
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM words_fts WHERE words_fts MATCH '\"画編集\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn バージョン_3_の_db_に提案の表が足され_状態の外の値は拒否する() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1, V2, V3]).unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 4);
        let insert = |state: &str| {
            conn.execute(
                "INSERT INTO proposals(name, entry, actor_kind, actor_name, at_ms, base_version,
                                       ops, touches, state)
                 VALUES ('p', 'mcp', 'mcp', 'claude', 0, 0, '[]', '[]', ?1)",
                [state],
            )
        };
        assert!(insert("pending").is_ok());
        assert!(insert("merged").is_err());
    }

    #[test]
    fn 新しいバージョンの_db_は拒否する() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[V1, V2_TEST]).unwrap();
        assert!(matches!(
            migrate(&mut conn, &[V1]),
            Err(MmError::Unsupported(_))
        ));
    }
}
