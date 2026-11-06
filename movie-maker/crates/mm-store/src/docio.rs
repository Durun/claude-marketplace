//! プロジェクトのデータと表の行の対応。プロジェクトのデータを行に分け、前の行との差分だけを書く。

use std::collections::{BTreeMap, HashMap};

use rusqlite::{params, Connection, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

use mm_doc::{Layer, Project};
use mm_types::{MmError, Result};

use crate::SqlExt;

/// 表 1 行ぶん。`key` は `table/id` か `layers/comp/id`。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DocRow {
    pub table: &'static str,
    pub id: String,
    pub comp: Option<String>,
    pub z: i64,
    pub json: String,
    /// 素材の `content_hash` と `path`（列に抜く）。
    pub hash: String,
    pub path: String,
    /// 検索の表に入れる (種類, 文字)。
    pub search: Vec<(&'static str, String)>,
}

/// 行のキーから行への写像。
pub(crate) type RowMap = BTreeMap<String, DocRow>;

fn row(table: &'static str, id: &str, json: String) -> DocRow {
    DocRow {
        table,
        id: id.to_string(),
        comp: None,
        z: 0,
        json,
        hash: String::new(),
        path: String::new(),
        search: Vec::new(),
    }
}

fn to_json<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)?)
}

/// 直前のプロジェクトのデータとその行。変わっていない要素は、直列化し直さずに行を使い回す。
pub(crate) struct Prev<'a> {
    pub project: &'a Project,
    pub rows: &'a RowMap,
}

impl Prev<'_> {
    /// `unchanged` が真なら前の行のコピー。
    fn reuse(&self, key: &str, unchanged: bool) -> Option<DocRow> {
        unchanged.then(|| self.rows.get(key).cloned()).flatten()
    }
}

/// プロジェクトのデータを行に分ける。
pub(crate) fn rows_of(p: &Project) -> Result<RowMap> {
    rows_after(None, p)
}

/// 前のプロジェクトのデータとの差だけを直列化して、プロジェクトのデータを行に分ける。
pub(crate) fn rows_after(prev: Option<&Prev>, p: &Project) -> Result<RowMap> {
    let mut rows = RowMap::new();
    let head = json!({ "schema_version": p.schema_version });
    rows.insert("project/1".into(), row("project", "1", head.to_string()));
    for a in &p.assets {
        let key = format!("assets/{}", a.id);
        let same = prev.is_some_and(|v| v.project.assets.iter().any(|o| o == a));
        let r = match prev.and_then(|v| v.reuse(&key, same)) {
            Some(r) => r,
            None => asset_row(a)?,
        };
        rows.insert(key, r);
    }
    for c in &p.compositions {
        insert_composition(&mut rows, prev, c)?;
    }
    for s in &p.sequences {
        let key = format!("sequences/{}", s.id);
        let same = prev.is_some_and(|v| v.project.sequences.iter().any(|o| o == s));
        let r = match prev.and_then(|v| v.reuse(&key, same)) {
            Some(r) => r,
            None => row("sequences", s.id.as_str(), to_json(s)?),
        };
        rows.insert(key, r);
    }
    for t in &p.render_targets {
        let key = format!("render_targets/{}", t.id);
        let same = prev.is_some_and(|v| v.project.render_targets.iter().any(|o| o == t));
        let r = match prev.and_then(|v| v.reuse(&key, same)) {
            Some(r) => r,
            None => row("render_targets", t.id.as_str(), to_json(t)?),
        };
        rows.insert(key, r);
    }
    Ok(rows)
}

fn asset_row(a: &mm_doc::Asset) -> Result<DocRow> {
    let mut r = row("assets", a.id.as_str(), to_json(a)?);
    r.hash = a.content_hash.clone();
    r.path = a.path.clone();
    r.search.push(("asset", a.path.clone()));
    Ok(r)
}

/// レイヤー以外の部分が同じか。
fn same_head(a: &mm_doc::Composition, b: &mm_doc::Composition) -> bool {
    (a.width, a.height, &a.fps, &a.duration, &a.markers)
        == (b.width, b.height, &b.fps, &b.duration, &b.markers)
}

fn composition_row(c: &mm_doc::Composition) -> Result<DocRow> {
    let mut value = serde_json::to_value(c)?;
    if let Some(obj) = value.as_object_mut() {
        obj.remove("layers");
    }
    let mut r = row("compositions", c.id.as_str(), value.to_string());
    r.search.push(("composition", c.id.to_string()));
    r.search
        .extend(c.markers.iter().map(|m| ("marker", m.name.clone())));
    Ok(r)
}

fn layer_row(comp: &mm_doc::Composition, z: usize, layer: &Layer) -> Result<DocRow> {
    let mut lr = row("layers", layer.id.as_str(), to_json(layer)?);
    lr.comp = Some(comp.id.to_string());
    lr.z = z as i64;
    lr.search.push(("layer", layer_text(layer)));
    Ok(lr)
}

fn insert_composition(
    rows: &mut RowMap,
    prev: Option<&Prev>,
    c: &mm_doc::Composition,
) -> Result<()> {
    let old = prev.and_then(|v| v.project.compositions.iter().find(|o| o.id == c.id));
    let key = format!("compositions/{}", c.id);
    let head = match prev.and_then(|v| v.reuse(&key, old.is_some_and(|o| same_head(o, c)))) {
        Some(r) => r,
        None => composition_row(c)?,
    };
    rows.insert(key, head);
    let old_layers: HashMap<&mm_types::LayerId, (usize, &Layer)> = old
        .map(|o| {
            o.layers
                .iter()
                .enumerate()
                .map(|(i, l)| (&l.id, (i, l)))
                .collect()
        })
        .unwrap_or_default();
    for (z, layer) in c.layers.iter().enumerate() {
        let key = format!("layers/{}/{}", c.id, layer.id);
        let same = old_layers
            .get(&layer.id)
            .is_some_and(|(oz, ol)| *oz == z && *ol == layer);
        let r = match prev.and_then(|v| v.reuse(&key, same)) {
            Some(r) => r,
            None => layer_row(c, z, layer)?,
        };
        rows.insert(key, r);
    }
    Ok(())
}

/// レイヤーの検索用の文字。ID、種類の名前、文字のパラメータ。
fn layer_text(layer: &Layer) -> String {
    let mut parts = vec![layer.id.to_string()];
    for node in layer.source.descendants() {
        parts.push(node.kind.clone());
        parts.extend(
            node.params
                .values()
                .filter_map(|p| p.value.as_text().map(str::to_string)),
        );
    }
    parts.join(" ")
}

/// 表の順（親を先に書く）。
const WRITE_ORDER: [&str; 6] = [
    "project",
    "assets",
    "compositions",
    "layers",
    "sequences",
    "render_targets",
];

/// `old` から `new` への差分を、このトランザクションの中で書く。
pub(crate) fn apply_diff(tx: &Transaction, old: &RowMap, new: &RowMap) -> Result<()> {
    for key in old.keys().rev().filter(|k| !new.contains_key(*k)) {
        delete_row(tx, &old[key])?;
    }
    for table in WRITE_ORDER {
        for (key, r) in new.iter().filter(|(_, r)| r.table == table) {
            if old.get(key) != Some(r) {
                upsert_row(tx, r)?;
            }
        }
    }
    Ok(())
}

/// 全行を書き直す（プロジェクトのデータのバージョンが古い DB を開いたとき）。
pub(crate) fn write_all(tx: &Transaction, new: &RowMap) -> Result<()> {
    for t in [
        "layers",
        "compositions",
        "assets",
        "sequences",
        "render_targets",
    ] {
        tx.execute(&format!("DELETE FROM {t}"), []).sql()?;
    }
    tx.execute("DELETE FROM search", []).sql()?;
    apply_diff(tx, &RowMap::new(), new)
}

fn delete_row(tx: &Transaction, r: &DocRow) -> Result<()> {
    let key = owner_key(r);
    match r.table {
        "layers" => tx.execute(
            "DELETE FROM layers WHERE comp_id = ?1 AND id = ?2",
            params![r.comp, r.id],
        ),
        "project" => return Ok(()),
        t => tx.execute(&format!("DELETE FROM {t} WHERE id = ?1"), [&r.id]),
    }
    .sql()?;
    tx.execute("DELETE FROM search WHERE owner = ?1", [&key])
        .sql()?;
    Ok(())
}

fn owner_key(r: &DocRow) -> String {
    match &r.comp {
        Some(c) => format!("{}/{c}/{}", r.table, r.id),
        None => format!("{}/{}", r.table, r.id),
    }
}

fn upsert_row(tx: &Transaction, r: &DocRow) -> Result<()> {
    match r.table {
        "project" => tx.execute(
            "INSERT INTO project(id, json) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json",
            [&r.json],
        ),
        "assets" => tx.execute(
            "INSERT INTO assets(id, content_hash, path, json) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET content_hash = excluded.content_hash,
               path = excluded.path, json = excluded.json",
            params![r.id, r.hash, r.path, r.json],
        ),
        "layers" => tx.execute(
            "INSERT INTO layers(comp_id, id, z, json) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(comp_id, id) DO UPDATE SET z = excluded.z, json = excluded.json",
            params![r.comp, r.id, r.z, r.json],
        ),
        t => tx.execute(
            &format!(
                "INSERT INTO {t}(id, json) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json"
            ),
            params![r.id, r.json],
        ),
    }
    .sql()?;
    let key = owner_key(r);
    tx.execute("DELETE FROM search WHERE owner = ?1", [&key])
        .sql()?;
    for (kind, text) in &r.search {
        tx.execute(
            "INSERT INTO search(text, kind, owner) VALUES (?1, ?2, ?3)",
            params![text, kind, key],
        )
        .sql()?;
    }
    Ok(())
}

fn json_rows(conn: &Connection, sql: &str) -> Result<Vec<Value>> {
    let mut st = conn.prepare(sql).sql()?;
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

/// 表からプロジェクトのデータを組み立てる。古いバージョンのプロジェクトのデータは `mm_doc::migrate` で今のバージョンに上げる。
pub(crate) fn read_project(conn: &Connection) -> Result<Project> {
    let head = json_rows(conn, "SELECT json FROM project WHERE id = 1")?
        .pop()
        .ok_or_else(|| MmError::Corrupt("project の行が無い".into()))?;
    let mut comps = json_rows(conn, "SELECT json FROM compositions ORDER BY id")?;
    for c in &mut comps {
        let id = c["id"].as_str().unwrap_or_default().to_string();
        let mut st = conn
            .prepare("SELECT json FROM layers WHERE comp_id = ?1 ORDER BY z")
            .sql()?;
        let layers: Vec<Value> = st
            .query_map([&id], |r| r.get::<_, String>(0))
            .sql()?
            .map(|t| Ok(serde_json::from_str(&t.sql()?)?))
            .collect::<Result<_>>()?;
        c["layers"] = Value::Array(layers);
    }
    let value = json!({
        "schema_version": head["schema_version"],
        "assets": json_rows(conn, "SELECT json FROM assets ORDER BY id")?,
        "compositions": comps,
        "sequences": json_rows(conn, "SELECT json FROM sequences ORDER BY id")?,
        "render_targets": json_rows(conn, "SELECT json FROM render_targets ORDER BY id")?,
    });
    let mut project: Project = serde_json::from_value(mm_doc::migrate(value)?)?;
    project.normalize();
    Ok(project)
}

/// DB に書かれているプロジェクトのデータのスキーマのバージョン。
pub(crate) fn stored_doc_version(conn: &Connection) -> Result<u32> {
    let head = json_rows(conn, "SELECT json FROM project WHERE id = 1")?
        .pop()
        .ok_or_else(|| MmError::Corrupt("project の行が無い".into()))?;
    head["schema_version"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| MmError::Corrupt("schema_version が読めない".into()))
}
