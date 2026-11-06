//! 素材の整理（`bins`）と文字起こしの直しの行。要素ごとに 1 行にし、操作 1 つで変わる行を小さく保つ。

use std::collections::HashMap;

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
    // 前の要素を ID の索引にして比べる（要素ごとに前の全部と比べると、要素数の 2 乗になる）。
    let old = prev.map(|v| v.project);
    let old_marks = index_by(old.map(|o| &o.bins.marks[..]), |m| m.asset.to_string());
    let old_selections = index_by(old.map(|o| &o.bins.selections[..]), |s| s.id.to_string());
    let old_topics = index_by(old.map(|o| &o.bins.topics[..]), |t| t.id.to_string());
    let old_edits = index_by(old.map(|o| &o.transcript_edits[..]), edit_key);
    for m in &p.bins.marks {
        let same = old_marks.get(m.asset.as_str()) == Some(&m);
        insert_item(rows, prev, ("marks", m.asset.as_str()), same, m, Vec::new())?;
    }
    for s in &p.bins.selections {
        let same = old_selections.get(s.id.as_str()) == Some(&s);
        let search = s.tags.iter().map(|t| ("tag", t.clone())).collect();
        insert_item(rows, prev, ("selections", s.id.as_str()), same, s, search)?;
    }
    for t in &p.bins.topics {
        let same = old_topics.get(t.id.as_str()) == Some(&t);
        let mut search = vec![("topic", t.title.clone())];
        search.extend(t.tags.iter().map(|tag| ("tag", tag.clone())));
        insert_item(rows, prev, ("topics", t.id.as_str()), same, t, search)?;
    }
    for e in &p.transcript_edits {
        let key = edit_key(e);
        let same = old_edits.get(key.as_str()) == Some(&e);
        insert_item(rows, prev, ("word_edits", &key), same, e, Vec::new())?;
    }
    Ok(())
}

/// 要素を `key` の文字で引く索引。前のプロジェクトのデータが無ければ空。
fn index_by<T>(items: Option<&[T]>, key: impl Fn(&T) -> String) -> HashMap<String, &T> {
    items
        .unwrap_or_default()
        .iter()
        .map(|item| (key(item), item))
        .collect()
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
