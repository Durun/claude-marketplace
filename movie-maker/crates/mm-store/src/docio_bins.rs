//! 素材の整理（`bins`）と文字起こしの直しの行。要素ごとに 1 行にし、操作 1 つで変わる行を小さく保つ。

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

use mm_doc::{Project, WordEdit};
use mm_types::Result;

use crate::docio::{json_rows, row, Prev, RowMap};

/// 整理と直しの表（`(id, json)` の形）。
pub(crate) const BIN_TABLES: [&str; 4] = ["marks", "selections", "topics", "word_edits"];

/// 直しの行の ID。素材の ID と単語の ID の組。
fn edit_key(e: &WordEdit) -> String {
    format!("{}/{}", e.asset, e.word)
}

/// 整理と直しを行に分けて `rows` に足す。前と同じ要素は前の行を使い回す。
pub(crate) fn insert_bins(rows: &mut RowMap, prev: Option<&Prev>, p: &Project) -> Result<()> {
    let old = prev.map(|v| v.project);
    for m in &p.bins.marks {
        let same = old.is_some_and(|o| o.bins.marks.contains(m));
        insert_item(rows, prev, ("marks", m.asset.as_str()), same, m, Vec::new())?;
    }
    for s in &p.bins.selections {
        let same = old.is_some_and(|o| o.bins.selections.contains(s));
        let search = s.tags.iter().map(|t| ("tag", t.clone())).collect();
        insert_item(rows, prev, ("selections", s.id.as_str()), same, s, search)?;
    }
    for t in &p.bins.topics {
        let same = old.is_some_and(|o| o.bins.topics.contains(t));
        let mut search = vec![("topic", t.title.clone())];
        search.extend(t.tags.iter().map(|tag| ("tag", tag.clone())));
        insert_item(rows, prev, ("topics", t.id.as_str()), same, t, search)?;
    }
    for e in &p.transcript_edits {
        let same = old.is_some_and(|o| o.transcript_edits.contains(e));
        insert_item(
            rows,
            prev,
            ("word_edits", &edit_key(e)),
            same,
            e,
            Vec::new(),
        )?;
    }
    Ok(())
}

fn insert_item<T: Serialize>(
    rows: &mut RowMap,
    prev: Option<&Prev>,
    (table, id): (&'static str, &str),
    unchanged: bool,
    item: &T,
    search: Vec<(&'static str, String)>,
) -> Result<()> {
    let key = format!("{table}/{id}");
    let reused = prev.and_then(|v| v.reuse(&key, unchanged));
    let r = match reused {
        Some(r) => r,
        None => {
            let mut r = row(table, id, serde_json::to_string(item)?);
            r.search = search;
            r
        }
    };
    rows.insert(key, r);
    Ok(())
}

/// 表から整理と直しを読み、プロジェクトのデータの JSON に置く。
pub(crate) fn read_bins(conn: &Connection, value: &mut Value) -> Result<()> {
    let read = |t: &str| json_rows(conn, &format!("SELECT json FROM {t} ORDER BY id"));
    value["bins"] = json!({
        "marks": read("marks")?,
        "selections": read("selections")?,
        "topics": read("topics")?,
    });
    value["transcript_edits"] = Value::Array(read("word_edits")?);
    Ok(())
}
