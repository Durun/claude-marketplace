//! アプリの代わりにテストの中で `mm-serve` を起こし、`mm op` / `undo` / `redo` / `frame` が
//! HTTP で転送されることを確かめる。テストのプロセスが書き手なので、`mm` は別のプロセスとして見る。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mm_doc::Project;
use mm_serve::{Client, Core, FrameRequest, FrameSource, Server};
use mm_store::Store;
use serde_json::json;
use tempfile::TempDir;

use super::common::{mm, mm_ok, sequence_json, stderr, stdout};

const NOW: i64 = 1_700_000_000_000;

/// PNG の署名と時刻だけを返す描画の代わり。
struct StubFrames;

impl FrameSource for StubFrames {
    fn frame_png(&self, _p: &Project, r: &FrameRequest) -> mm_types::Result<Vec<u8>> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(r.time.to_f64().to_string().bytes());
        Ok(bytes)
    }
}

/// 書き手として開いた空のプロジェクトと、その HTTP サーバー。
fn app(tmp: &TempDir) -> (PathBuf, Server) {
    let dir = tmp.path().join("p");
    let store = Store::create(&dir, &Project::new(), NOW).unwrap();
    let core = Core::new(store, Some(Arc::new(StubFrames)));
    (dir, Server::start(core).unwrap())
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn mm_op_は書き手のアプリへ_http_で転送され_変更が_sse_で届く() {
    let tmp = TempDir::new().unwrap();
    let (dir, server) = app(&tmp);
    let client = Client::new(server.port(), server.token());
    let mut events = client.events().unwrap();
    events.next_event().unwrap();

    let out = mm_ok(&["op", path(&dir), "addSequence", "--json", &sequence_json()]);
    assert!(stdout(&out).contains("アプリへ転送"), "{}", stdout(&out));

    let change = events.next_event().unwrap().unwrap();
    assert_eq!(
        (change.event.as_str(), &change.data["version"]),
        ("change", &json!(1))
    );
    assert_eq!(change.data["actor"], "cli");
    assert_eq!(
        client.get("/project").unwrap().json().unwrap()["version"],
        1
    );

    // 古いバージョンを元にした操作は、touches が重なれば断られる。
    mm_ok(&[
        "op",
        path(&dir),
        "ripple",
        "--cut",
        "c1",
        "--edge",
        "out",
        "--by",
        "-0.4",
    ]);
    let stale = mm(&[
        "op",
        path(&dir),
        "ripple",
        "--cut",
        "c1",
        "--edge",
        "out",
        "--by",
        "-0.4",
        "--base",
        "1",
    ]);
    assert!(!stale.status.success());
    assert!(
        stderr(&stale).contains("触る所が重なる"),
        "{}",
        stderr(&stale)
    );
}

#[test]
fn mm_undo_と_redo_も転送される() {
    let tmp = TempDir::new().unwrap();
    let (dir, server) = app(&tmp);
    mm_ok(&["op", path(&dir), "addSequence", "--json", &sequence_json()]);
    let undo = mm_ok(&["undo", path(&dir)]);
    assert!(stdout(&undo).contains("アプリへ転送"), "{}", stdout(&undo));
    let none = mm_ok(&["undo", path(&dir)]);
    assert!(stdout(&none).contains("取り消すものが無い"));
    mm_ok(&["redo", path(&dir)]);
    let client = Client::new(server.port(), server.token());
    assert_eq!(
        client.get("/project").unwrap().json().unwrap()["version"],
        3
    );
}

#[test]
fn 合い言葉が消えていれば転送せず_理由を言う() {
    let tmp = TempDir::new().unwrap();
    let (dir, _server) = app(&tmp);
    std::fs::remove_file(Store::writer_token_file(&dir).unwrap()).unwrap();
    let out = mm(&["op", path(&dir), "addSequence", "--json", &sequence_json()]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("HTTP サーバーが無い"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn mm_frame_は動いているアプリに描いてもらう() {
    let tmp = TempDir::new().unwrap();
    let (dir, _server) = app(&tmp);
    let png = tmp.path().join("x.png");
    mm_ok(&["frame", path(&dir), "3/2", "-o", path(&png)]);
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    assert!(String::from_utf8_lossy(&bytes).ends_with("1.5"));
}
