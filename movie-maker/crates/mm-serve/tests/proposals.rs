//! 提案のエンドポイントの結合テスト。MCP から提案を出し、人の経路（cli）で承認・却下する。

use std::sync::Arc;

use mm_doc::Project;
use mm_serve::{Client, Core, FrameRequest, FrameSource, Server};
use mm_store::Store;
use mm_types::Result;
use serde_json::{json, Value};

const NOW: i64 = 1_700_000_000_000;

/// 描いたプロジェクトのデータの sequence の長さ（カットの数）を PNG の後ろに付ける描画の代わり。
struct StubFrames;

impl FrameSource for StubFrames {
    fn frame_png(&self, project: &Project, _request: &FrameRequest) -> Result<Vec<u8>> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let cuts = project.sequences.first().map_or(0, |s| s.cuts.len());
        bytes.extend(cuts.to_string().bytes());
        Ok(bytes)
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    _server: Server,
    client: Client,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::create(&tmp.path().join("p"), &Project::new(), NOW).unwrap();
    let server = Server::start(Core::new(store, Some(Arc::new(StubFrames)))).unwrap();
    let client = Client::new(server.port(), server.token());
    Rig {
        _tmp: tmp,
        _server: server,
        client,
    }
}

fn cut(id: &str, secs: i64) -> Value {
    json!({
        "id": id,
        "source": {"kind": "solid", "version": 1, "params": {}, "inputs": []},
        "in": {"num": 0, "den": 1},
        "out": {"num": secs, "den": 1},
    })
}

fn add_sequence() -> Value {
    json!({"op": "addSequence", "sequence": {"id": "s1", "cuts": [cut("c1", 4), cut("c2", 2)]}})
}

fn split(cut: &str, at: i64) -> Value {
    json!({"op": "split", "cut": cut, "at": {"num": at, "den": 1}, "new_cut": format!("{cut}b")})
}

fn propose(c: &Client, ops: Vec<Value>) -> Value {
    let body = json!({
        "txn": {"name": "Claude の対応", "ops": ops},
        "entry": "mcp", "responds_to": "c-9",
    });
    let res = c.post("/proposals", &body).unwrap();
    assert_eq!(res.status, 200, "{}", res.error_message());
    res.json().unwrap()
}

fn version(c: &Client) -> Value {
    c.get("/project").unwrap().json().unwrap()["version"].clone()
}

#[test]
fn mcp_の提案は保留され_範囲と操作が見え_cli_で承認すると適用される() {
    let r = rig();
    let c = &r.client;
    c.post("/ops", &json!({"op": add_sequence()})).unwrap();
    let p = propose(c, vec![split("c1", 1), split("c2", 1)]);
    assert_eq!(p["state"], "pending");
    assert_eq!(
        (p["actor"].as_str(), p["actor_name"].as_str()),
        (Some("mcp"), Some("claude"))
    );
    assert_eq!(p["responds_to"], "c-9");
    assert_eq!(p["ops"][1]["op"]["op"], "split");
    assert_eq!(p["scope"]["sequences"][0]["id"], "s1");
    assert_eq!(version(c), 1, "保留中はデータを変えない");

    let id = p["id"].as_i64().unwrap();
    let list = c.get("/proposals").unwrap().json().unwrap();
    assert_eq!(list["proposals"][0]["id"], id);

    let proposed = c.get(&format!("/frame?t=0&proposal={id}")).unwrap().body;
    let one = c
        .get(&format!("/frame?t=0&proposal={id}&ops=1"))
        .unwrap()
        .body;
    let original = c.get("/frame?t=0").unwrap().body;
    assert!(proposed.ends_with(b"4") && one.ends_with(b"3") && original.ends_with(b"2"));

    let res = c
        .post(&format!("/proposals/{id}/approve"), &json!({"ops": [1]}))
        .unwrap();
    assert_eq!(res.status, 200, "{}", res.error_message());
    let body = res.json().unwrap();
    assert_eq!(body["proposal"]["state"], "approved");
    assert_eq!(body["proposal"]["approved_ops"], json!([1]));
    assert_eq!(body["change"]["actor"], "mcp", "発行元は提案した者");
    assert!(body["proposal"]["undoable"].as_bool().unwrap());

    let undo = c.post("/undo", &json!({})).unwrap().json().unwrap();
    assert_eq!(undo["change"]["of"]["txn_id"], body["change"]["txn_id"]);
    let p = c.get(&format!("/proposals/{id}")).unwrap().json().unwrap();
    assert_eq!(
        (p["applied"].as_bool(), p["undoable"].as_bool()),
        (Some(false), Some(false))
    );
}

#[test]
fn mcp_の経路からは承認も却下もできない() {
    let r = rig();
    let c = &r.client;
    c.post("/ops", &json!({"op": add_sequence()})).unwrap();
    let id = propose(c, vec![split("c1", 1)])["id"].as_i64().unwrap();
    for action in ["approve", "reject"] {
        let res = c
            .post(
                &format!("/proposals/{id}/{action}"),
                &json!({"entry": "mcp"}),
            )
            .unwrap();
        assert_eq!(res.status, 403);
        assert_eq!(res.json().unwrap()["error"]["code"], "forbidden");
    }
}

#[test]
fn 却下はデータを変えず_保留に戻せ_通知が_sse_で届く() {
    let r = rig();
    let c = &r.client;
    c.post("/ops", &json!({"op": add_sequence()})).unwrap();
    let mut events = c.events().unwrap();
    events.next_event().unwrap();
    let id = propose(c, vec![split("c1", 1)])["id"].as_i64().unwrap();
    let ev = events.next_event().unwrap().unwrap();
    assert_eq!(
        (ev.event.as_str(), &ev.data["state"]),
        ("proposal", &json!("pending"))
    );

    let p = c
        .post(&format!("/proposals/{id}/reject"), &json!({}))
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(p["state"], "rejected");
    assert_eq!(version(c), 1);
    let ev = events.next_event().unwrap().unwrap();
    assert_eq!(ev.data["state"], "rejected");

    let p = c
        .post(&format!("/proposals/{id}/reopen"), &json!({}))
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(p["state"], "pending");
}

#[test]
fn 後の編集と重なる提案は_conflict_を出し_承認は_409() {
    let r = rig();
    let c = &r.client;
    c.post("/ops", &json!({"op": add_sequence()})).unwrap();
    let id = propose(c, vec![split("c1", 1)])["id"].as_i64().unwrap();
    let human = json!({"op": "split", "cut": "c1", "at": {"num": 3, "den": 1}, "new_cut": "c1x"});
    c.post("/ops", &json!({ "op": human })).unwrap();
    let p = c.get(&format!("/proposals/{id}")).unwrap().json().unwrap();
    assert!(
        p["conflict"]
            .as_array()
            .is_some_and(|a| a.contains(&json!("c1"))),
        "{p}"
    );
    let res = c
        .post(&format!("/proposals/{id}/approve"), &json!({}))
        .unwrap();
    assert_eq!(res.status, 409);
    assert_eq!(c.get("/proposals/99").unwrap().status, 400);
}
