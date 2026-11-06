//! `versions/` の JSON の書き出しと取り込み。同じ文書からは同じバイト列を作る。
//!
//! キーは辞書順、配列は文書の順（ID の列は `normalize` 済み）、インデントは 2、改行は LF、末尾に改行 1 つ。
//! 時刻は有理数のまま、浮動小数は最短で往復する表記になる。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use mm_doc::Project;
use mm_types::{MmError, Result};

/// プロジェクトの下の書き出し先。
pub const VERSIONS_DIR: &str = "versions";
const INDEX_FILE: &str = "project.json";
const REVIEW_FILE: &str = "review.json";
/// ID ごとのファイルを置く下のフォルダー。
const PER_ID_DIRS: [&str; 3] = ["comps", "assets", "sequences"];

/// 書き出しの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// 全ファイル（相対パスと中身）の SHA-256（16 進）。`versions` の表に残す。
    pub hash: String,
    /// 書き出したファイルの相対パス。
    pub files: Vec<String>,
    /// 中身が変わって実際に書いたファイルの数。
    pub written: usize,
}

/// キーを辞書順にそろえた JSON の文字列にする。
pub(crate) fn canonical(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(&sorted(v)).unwrap_or_default();
    s.push('\n');
    s
}

fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                out.insert(k.clone(), sorted(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// ファイル名に使えない文字を `%XX` にする。大文字と小文字を区別しないファイルシステム（NTFS、APFS）で
/// `Comp` と `comp` が同じファイルにならないよう、大文字も符号化する。
fn file_stem(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn path_of(dir: &str, id: &str) -> String {
    format!("{dir}/{}.json", file_stem(id))
}

fn id_of(v: &Value) -> Result<String> {
    v["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| MmError::Corrupt("id の無い要素".into()))
}

/// 書き出すファイル全部を作る。相対パスから中身へ。
pub(crate) fn render_files(
    project: &Project,
    comments: &[Value],
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut p = project.clone();
    p.normalize();
    let whole = serde_json::to_value(&p)?;
    let mut files = BTreeMap::new();
    let mut index = json!({ "schema_version": p.schema_version });
    for (field, dir) in [
        ("compositions", "comps"),
        ("assets", "assets"),
        ("sequences", "sequences"),
    ] {
        let items = whole[field].as_array().cloned().unwrap_or_default();
        let ids = items.iter().map(id_of).collect::<Result<Vec<_>>>()?;
        for (id, item) in ids.iter().zip(&items) {
            files.insert(path_of(dir, id), canonical(item).into_bytes());
        }
        index[field] = json!(ids);
    }
    index["render_targets"] = whole["render_targets"].clone();
    files.insert(INDEX_FILE.into(), canonical(&index).into_bytes());
    let mut sorted_comments = comments.to_vec();
    sorted_comments.sort_by_key(|c| c["id"].as_str().unwrap_or_default().to_string());
    let review = json!({ "comments": sorted_comments });
    files.insert(REVIEW_FILE.into(), canonical(&review).into_bytes());
    Ok(files)
}

/// 全ファイルのハッシュ。
pub(crate) fn hash_files(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut h = Sha256::new();
    for (path, bytes) in files {
        h.update(path.as_bytes());
        h.update([0]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// `versions/` に上書きする。変わらないファイルは触らず、文書から消えた ID のファイルは消す。
pub(crate) fn write_files(versions: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<usize> {
    fs::create_dir_all(versions)?;
    let mut written = 0;
    for (rel, bytes) in files {
        let path = versions.join(rel);
        if fs::read(&path).ok().as_deref() == Some(bytes) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
        written += 1;
    }
    for dir in PER_ID_DIRS {
        remove_stale(&versions.join(dir), dir, files)?;
    }
    Ok(written)
}

fn remove_stale(dir: &Path, name: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in rd {
        let entry = entry?;
        let rel = format!("{name}/{}", entry.file_name().to_string_lossy());
        if !files.contains_key(&rel) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)
        .map_err(|e| MmError::NotFound(format!("{}: {e}", path.display())))?;
    Ok(serde_json::from_str(&text)?)
}

/// `versions/` から文書とコメントを読む。古いスキーマの版は `mm_doc::load` が今の版に上げる。
pub(crate) fn read_files(versions: &Path) -> Result<(Project, Vec<Value>)> {
    let index = read_json(&versions.join(INDEX_FILE))?;
    let mut whole = json!({
        "schema_version": index["schema_version"],
        "render_targets": index["render_targets"],
    });
    for (field, dir) in [
        ("compositions", "comps"),
        ("assets", "assets"),
        ("sequences", "sequences"),
    ] {
        let ids = index[field].as_array().cloned().unwrap_or_default();
        let items = ids
            .iter()
            .map(|id| read_json(&versions.join(path_of(dir, id.as_str().unwrap_or_default()))))
            .collect::<Result<Vec<_>>>()?;
        whole[field] = Value::Array(items);
    }
    let project = mm_doc::load(&whole.to_string())?;
    let review = read_json(&versions.join(REVIEW_FILE))?;
    let comments = review["comments"].as_array().cloned().unwrap_or_default();
    Ok((project, comments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn キーは辞書順で入れ子も整う() {
        let v = json!({ "b": 1, "a": { "z": [ {"y": 1, "x": 2} ], "c": null } });
        let s = canonical(&v);
        assert!(s.find("\"a\"").unwrap() < s.find("\"b\"").unwrap());
        assert!(s.find("\"x\"").unwrap() < s.find("\"y\"").unwrap());
        assert!(s.ends_with("}\n") && !s.contains('\r'));
    }

    #[test]
    fn ファイル名に使えない_id_は符号化する() {
        assert_eq!(file_stem("a/b c"), "a%2Fb%20c");
        assert_eq!(file_stem("c-1_x.v"), "c-1_x.v");
        assert_ne!(file_stem("Comp"), file_stem("comp"));
        assert_eq!(file_stem("Comp"), "%43omp");
    }
}
