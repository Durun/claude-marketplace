//! 命令、取り消し、export / import の往復、diff、ロック、競合を実行ファイル越しに確かめる。

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{mm, mm_ok, sequence_json, stage0_json, stderr, stdout};
use tempfile::TempDir;

/// 並び s1（カット c1 は 4 秒、c2 は 2 秒）を足した空のプロジェクト。版は 1。
fn project_with_sequence(tmp: &TempDir) -> PathBuf {
    let dir = tmp.path().join("p");
    mm_ok(&["new", dir.to_str().unwrap()]);
    mm_ok(&[
        "op",
        dir.to_str().unwrap(),
        "addSequence",
        "--json",
        &sequence_json(),
    ]);
    dir
}

/// `versions/` の全ファイル（相対パスと中身）。
fn read_tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn 命令から取り消しとやり直しと_export_import_の往復() {
    let tmp = tempfile::tempdir().unwrap();
    let p = project_with_sequence(&tmp);
    let ps = p.to_str().unwrap();
    mm_ok(&[
        "op", ps, "ripple", "--cut", "c1", "--edge", "out", "--by", "-0.4",
    ]);
    mm_ok(&[
        "op", ps, "ripple", "--cut", "c2", "--edge", "out", "--by", "1/2",
    ]);
    mm_ok(&["undo", ps]);
    mm_ok(&["undo", ps]);
    mm_ok(&["redo", ps]);
    let log = stdout(&mm_ok(&["log", ps]));
    assert!(log.contains("取消"), "{log}");

    mm_ok(&["export", ps]);
    let versions = p.join("versions");
    let q = tmp.path().join("q");
    mm_ok(&["import", q.to_str().unwrap(), versions.to_str().unwrap()]);
    mm_ok(&["export", q.to_str().unwrap()]);
    assert_eq!(read_tree(&versions), read_tree(&q.join("versions")));
}

#[test]
fn diff_は意味の差分を人が読める文で出す() {
    let tmp = tempfile::tempdir().unwrap();
    let p = project_with_sequence(&tmp);
    let ps = p.to_str().unwrap();
    mm_ok(&[
        "op", ps, "ripple", "--cut", "c1", "--edge", "out", "--by", "-0.4",
    ]);
    mm_ok(&[
        "op", ps, "ripple", "--cut", "c2", "--edge", "out", "--by", "0.5",
    ]);
    mm_ok(&[
        "op", ps, "ripple", "--cut", "c2", "--edge", "out", "--by", "0.5",
    ]);
    mm_ok(&["undo", ps]);

    let out = stdout(&mm_ok(&["diff", ps, "1", "head"]));
    assert!(out.contains("カット c1 の尻を 0.4 秒詰めた"), "{out}");
    assert!(out.contains("カット c2 の尻を 0.5 秒伸ばした"), "{out}");
    // 取り消した 3 つ目は出ない。
    assert_eq!(out.matches("c2").count(), 1, "{out}");

    mm_ok(&["export", ps, "--name", "v1"]);
    let named = stdout(&mm_ok(&["diff", ps, "v1", "head"]));
    assert!(named.contains("0 件"), "{named}");
}

#[test]
fn ロックした所に触る命令は断られ_文書は変わらない() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    mm_ok(&[
        "import",
        dir.to_str().unwrap(),
        stage0_json().to_str().unwrap(),
    ]);
    let ps = dir.to_str().unwrap();
    mm_ok(&["op", ps, "lockLayer", "--layer", "l-text", "--lock", "true"]);
    let before = stdout(&mm_ok(&["log", ps]));

    let out = mm(&[
        "op",
        ps,
        "setParam",
        "--target",
        "layer:l-text:opacity",
        "--json",
        r#"{"value":{"type":"number","value":0.5}}"#,
    ]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("ロック"), "{}", stderr(&out));
    assert_eq!(stdout(&mm_ok(&["log", ps])), before);

    // ロックの付いていないレイヤーは動かせる。
    mm_ok(&[
        "op",
        ps,
        "setParam",
        "--target",
        "layer:l-footage:opacity",
        "--json",
        r#"{"value":{"type":"number","value":0.5}}"#,
    ]);
}

#[test]
fn 古い版を元にした命令は触る所が重なれば断られ_重ならなければ通る() {
    let tmp = tempfile::tempdir().unwrap();
    let p = project_with_sequence(&tmp);
    let ps = p.to_str().unwrap();
    mm_ok(&[
        "op", ps, "ripple", "--cut", "c1", "--edge", "out", "--by", "-0.4", "--base", "1",
    ]);

    // 版 1 を元にして c1 を触ると、版 2 の ripple と重なる。
    let out = mm(&[
        "op", ps, "ripple", "--cut", "c1", "--edge", "out", "--by", "1", "--base", "1",
    ]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("重なる"), "{}", stderr(&out));
    // c2 は版 1 の後に誰も触っていない。
    mm_ok(&[
        "op", ps, "ripple", "--cut", "c2", "--edge", "out", "--by", "1", "--base", "1",
    ]);
    // 今の版より先は断る。
    let out = mm(&[
        "op", ps, "ripple", "--cut", "c2", "--edge", "out", "--by", "1", "--base", "99",
    ]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("先"), "{}", stderr(&out));
}

#[test]
fn txn_ファイルの命令は全部当たるか全部当たらない() {
    let tmp = tempfile::tempdir().unwrap();
    let p = project_with_sequence(&tmp);
    let ps = p.to_str().unwrap();
    let ripple = |cut: &str| serde_json::json!({"op": "ripple", "cut": cut, "edge": "out", "by": {"num": 1, "den": 2}});
    let good = tmp.path().join("good.json");
    fs::write(
        &good,
        serde_json::json!({"name": "まとめて", "ops": [ripple("c1"), ripple("c2")]}).to_string(),
    )
    .unwrap();
    mm_ok(&["op", ps, "--txn", good.to_str().unwrap()]);
    let log = stdout(&mm_ok(&["log", ps]));
    assert!(
        log.contains("まとめて") && log.contains("ripple, ripple"),
        "{log}"
    );

    // 2 つ目が失敗するまとまりは、1 つ目も残らない。
    let bad = tmp.path().join("bad.json");
    fs::write(
        &bad,
        serde_json::json!({"name": "失敗", "ops": [ripple("c1"), ripple("none")]}).to_string(),
    )
    .unwrap();
    assert!(!mm(&["op", ps, "--txn", bad.to_str().unwrap()])
        .status
        .success());
    assert_eq!(stdout(&mm_ok(&["log", ps])), log);

    // 1 回で取り消せる。
    mm_ok(&["undo", ps]);
    assert!(stdout(&mm_ok(&["diff", ps, "1", "head"])).contains("0 件"));
}

#[test]
fn schema_は全部の命令を含み_知らない命令は使える名前を添えて断る() {
    let schema = stdout(&mm_ok(&["op", "--schema"]));
    let value: serde_json::Value = serde_json::from_str(&schema).unwrap();
    assert_eq!(
        value["oneOf"].as_array().unwrap().len(),
        mm_ops::Op::NAMES.len()
    );

    let tmp = tempfile::tempdir().unwrap();
    let p = project_with_sequence(&tmp);
    let out = mm(&["op", p.to_str().unwrap(), "nope"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("ripple"), "{}", stderr(&out));
}
