//! `versions/` の書き出しと取り込み。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::json;

use mm_store::Store;

use crate::common::{project_with, txn, NOW};

fn read_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    out
}

fn prepared(dir: &Path) -> Store {
    let mut s = Store::create(dir, &project_with(0), NOW).unwrap();
    s.commit(&project_with(3), &txn("t1", &[0, 1, 2])).unwrap();
    s.put_comment(&json!({ "id": "r2", "state": "done", "comp_id": "c" }))
        .unwrap();
    s.put_comment(&json!({ "id": "r1", "state": "open", "text": "尻を詰める" }))
        .unwrap();
    s
}

#[test]
fn 書き出し_取り込み_書き出しでバイトが同じ() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let first = prepared(a.path());
    let report1 = first.export_versions().unwrap();

    let imported =
        Store::import_versions(b.path(), &a.path().join("versions"), false, NOW).unwrap();
    assert_eq!(imported.project(), first.project());
    assert_eq!(imported.doc_version(), 0, "命令の記録は入らない");
    let report2 = imported.export_versions().unwrap();

    assert_eq!(report1.hash, report2.hash);
    assert_eq!(
        read_tree(&a.path().join("versions")),
        read_tree(&b.path().join("versions"))
    );
    assert!(report1.files.contains(&"comps/c.json".to_string()));
    assert!(report1.files.contains(&"review.json".to_string()));
}

#[test]
fn 変わらない書き出しはファイルを触らない() {
    let tmp = tempfile::tempdir().unwrap();
    let s = prepared(tmp.path());
    assert!(s.export_versions().unwrap().written > 0);
    assert_eq!(s.export_versions().unwrap().written, 0);
}

#[test]
fn 文書から消えた_id_のファイルは消える() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = prepared(tmp.path());
    s.export_versions().unwrap();
    let mut p = s.project().clone();
    p.compositions.clear();
    s.commit(&p, &txn("t2", &[9])).unwrap();
    s.export_versions().unwrap();
    assert!(!tmp.path().join("versions/comps/c.json").exists());
}

#[test]
fn json_は決定的な形で書かれる() {
    let tmp = tempfile::tempdir().unwrap();
    let s = prepared(tmp.path());
    s.export_versions().unwrap();
    let text = fs::read_to_string(tmp.path().join("versions/comps/c.json")).unwrap();
    assert!(text.ends_with("}\n") && !text.contains('\r'));
    assert!(text.starts_with("{\n  \"duration\""), "{text}");
    assert!(text.contains("\"num\""));
}

#[test]
fn 名前付きの版は表に残り_import_は置き換え前の写しを残す() {
    let tmp = tempfile::tempdir().unwrap();
    let s = prepared(tmp.path());
    let report = s
        .export_named_version("v1", "cli", NOW, Some("abc".into()))
        .unwrap();
    let v = s.version_by_name("v1").unwrap().unwrap();
    assert_eq!((v.doc_version, v.ops_seq, v.hash), (3, 3, report.hash));
    drop(s);

    let versions = tmp.path().join("versions");
    assert!(Store::import_versions(tmp.path(), &versions, false, NOW).is_err());
    let again = Store::import_versions(tmp.path(), &versions, true, NOW + 1000).unwrap();
    assert_eq!(again.doc_version(), 0);
    assert!(again.txns_between(0, 100).unwrap().is_empty());
    assert_eq!(fs::read_dir(tmp.path().join("backups")).unwrap().count(), 1);
}
