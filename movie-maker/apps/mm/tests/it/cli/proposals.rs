//! `mm proposal`: 直接開いたときと、書き手のアプリへ HTTP で転送したときの、提案・承認・却下。

use std::path::Path;

use mm_doc::Project;
use mm_serve::{Client, Core, Server};
use mm_store::Store;
use serde_json::json;
use tempfile::TempDir;

use super::common::{mm, mm_ok, sequence_json, stdout};

const NOW: i64 = 1_700_000_000_000;

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn ripple(cut: &str) -> [&str; 6] {
    ["--cut", cut, "--edge", "out", "--by", "-0.5"]
}

#[test]
fn 直接開いて提案し_一部だけ承認し_取り消せる() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("p");
    let p = path(&dir);
    mm_ok(&["new", p]);
    mm_ok(&["op", p, "addSequence", "--json", &sequence_json()]);
    let txn = tmp.path().join("txn.json");
    let ops = json!({"name": "詰める", "ops": [
        {"op": "ripple", "cut": "c1", "edge": "out", "by": {"num": -1, "den": 2}},
        {"op": "ripple", "cut": "c2", "edge": "out", "by": {"num": -1, "den": 2}},
    ]});
    std::fs::write(&txn, ops.to_string()).unwrap();
    let out = stdout(&mm_ok(&[
        "proposal",
        "add",
        p,
        "--txn",
        path(&txn),
        "--responds-to",
        "c-1",
    ]));
    assert!(
        out.contains("提案 1  保留  詰める") && out.contains("対応: c-1"),
        "{out}"
    );
    assert!(
        stdout(&mm_ok(&["log", p])).lines().count() == 1,
        "保留中は記録に無い"
    );

    let out = stdout(&mm_ok(&["proposal", "approve", p, "1", "--ops", "1"]));
    assert!(
        out.contains("バージョン 2") && out.contains("0: ") && out.contains("（承認しなかった）"),
        "{out}"
    );
    let out = stdout(&mm_ok(&["proposal", "list", p, "--all"]));
    assert!(out.contains("承認（適用中）"), "{out}");
    mm_ok(&["undo", p]);
    let out = stdout(&mm_ok(&["proposal", "list", p, "--all"]));
    assert!(out.contains("承認（取り消し済み）"), "{out}");
    assert!(stdout(&mm_ok(&["proposal", "list", p])).contains("提案は無い"));
}

#[test]
fn 却下と保留に戻すはデータを変えない() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("p");
    let p = path(&dir);
    mm_ok(&["new", p]);
    mm_ok(&["op", p, "addSequence", "--json", &sequence_json()]);
    let mut args = vec!["proposal", "add", p, "ripple"];
    args.extend(ripple("c1"));
    mm_ok(&args);
    assert!(stdout(&mm_ok(&["proposal", "reject", p, "1"])).contains("却下"));
    assert!(!mm(&["proposal", "approve", p, "1"]).status.success());
    assert!(stdout(&mm_ok(&["proposal", "reopen", p, "1"])).contains("保留"));
    assert_eq!(stdout(&mm_ok(&["log", p])).lines().count(), 1);
}

#[test]
fn アプリが書き手なら提案と承認は_http_で転送される() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("p");
    let store = Store::create(&dir, &Project::new(), NOW).unwrap();
    let server = Server::start(Core::new(store, None)).unwrap();
    let client = Client::new(server.port(), server.token());
    let p = path(&dir);
    mm_ok(&["op", p, "addSequence", "--json", &sequence_json()]);
    let mut args = vec!["proposal", "add", p, "ripple"];
    args.extend(ripple("c2"));
    assert!(stdout(&mm_ok(&args)).contains("アプリへ転送"));
    let list = client.get("/proposals").unwrap().json().unwrap();
    assert_eq!(list["proposals"][0]["state"], "pending");
    assert_eq!(list["proposals"][0]["actor"], "cli");

    assert!(stdout(&mm_ok(&["proposal", "approve", p, "1"])).contains("バージョン 2"));
    let view = client.get("/proposals/1").unwrap().json().unwrap();
    assert_eq!(view["state"], "approved");
}
