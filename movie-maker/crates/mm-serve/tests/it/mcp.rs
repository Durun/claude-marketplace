//! `POST /mcp`（MCP、ステートレス、JSON 応答）の結合テスト。
//! 段階 0 のフィクスチャを書き手で開いた実際の待ち受けに、標準の `TcpStream` のクライアントでつなぐ。

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use mm_doc::Project;
use mm_serve::{Client, Core, FrameRequest, FrameSource, Server};
use mm_store::Store;
use mm_types::Result;
use serde_json::{json, Value};

const NOW: i64 = 1_700_000_000_000;
/// 操作でないツールの名前（名前の順）。操作のツールは mm-ops の一覧から取るので、操作を足しても書き直さない。
const NON_OP_TOOLS: [&str; 7] = [
    "frame",
    "project",
    "propose",
    "proposals",
    "redo",
    "transaction",
    "undo",
];

struct StubFrames;

impl FrameSource for StubFrames {
    fn frame_png(&self, _project: &Project, _request: &FrameRequest) -> Result<Vec<u8>> {
        Ok(b"\x89PNG\r\n\x1a\n".to_vec())
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    dir: std::path::PathBuf,
    server: Server,
    client: Client,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/stage0/project.json");
    let project: Project = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let dir = tmp.path().join("p");
    let store = Store::create(&dir, &project, NOW).unwrap();
    let server = Server::start(Core::new(store, Some(Arc::new(StubFrames)))).unwrap();
    let client = Client::new(server.port(), server.token());
    Rig {
        _tmp: tmp,
        dir,
        server,
        client,
    }
}

fn rpc(r: &Rig, method: &str, params: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let res = r.client.post("/mcp", &body).unwrap();
    assert_eq!(res.status, 200, "{}", res.error_message());
    res.json().unwrap()
}

/// `tools/call` の `result`。
fn call(r: &Rig, name: &str, arguments: Value) -> Value {
    let res = rpc(
        r,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    );
    res["result"].clone()
}

fn text_json(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn marker(id: &str, secs: i64) -> Value {
    json!({"composition": "main", "marker": {"id": id, "time": {"num": secs, "den": 1}, "name": "m"}})
}

#[test]
fn initialize_は要求のバージョンを返し_ツールを名乗る() {
    let r = rig();
    let res = rpc(&r, "initialize", json!({"protocolVersion": "2025-03-26"}));
    let result = &res["result"];
    assert_eq!(res["jsonrpc"], "2.0");
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["capabilities"], json!({"tools": {}}));
    assert_eq!(result["serverInfo"]["name"], "movie-maker");
    assert!(!result["instructions"].as_str().unwrap().is_empty());

    let none = rpc(&r, "initialize", json!({}));
    assert_eq!(none["result"]["protocolVersion"], "2025-06-18");
    let unknown = rpc(&r, "initialize", json!({"protocolVersion": "1999-01-01"}));
    assert_eq!(unknown["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn propose_は保留するだけで_mcp_からは承認できず_frame_で提案を適用したフレームを見られる() {
    let r = rig();
    let ops =
        json!([{"op": "addMarker", "composition": "main", "marker": marker("p", 1)["marker"]}]);
    let result = call(
        &r,
        "propose",
        json!({"name": "提案", "ops": ops, "responds_to": "c-2"}),
    );
    assert!(result.get("isError").is_none(), "{result}");
    let proposal = text_json(&result);
    assert_eq!(proposal["state"], "pending");
    assert_eq!(proposal["actor"], "mcp");
    let id = proposal["id"].as_i64().unwrap();
    assert_eq!(
        r.client.get("/project").unwrap().json().unwrap()["version"],
        0
    );

    let listed = text_json(&call(&r, "proposals", json!({})));
    assert_eq!(listed["proposals"][0]["id"], id);
    let preview = call(&r, "frame", json!({"t": "1", "proposal": id, "ops": [0]}));
    assert_eq!(preview["content"][0]["type"], "image", "{preview}");
    let stray = call(&r, "frame", json!({"t": "1", "ops": [0]}));
    assert_eq!(stray["isError"], true);

    let approve = r
        .client
        .post(
            &format!("/proposals/{id}/approve"),
            &json!({"entry": "mcp"}),
        )
        .unwrap();
    assert_eq!(approve.status, 403);
    let names: Vec<String> = rpc(&r, "tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(!names
        .iter()
        .any(|n| n.contains("approve") || n.contains("reject")));
}

fn refs(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            if let Some(r) = m.get("$ref").and_then(Value::as_str) {
                out.insert(r.to_owned());
            }
            m.values().for_each(|x| refs(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
        _ => {}
    }
}

#[test]
fn tools_list_は全部の操作と操作でないツールを返し_参照は_defs_で解決する() {
    let r = rig();
    let tools = rpc(&r, "tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .clone();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(unique.len(), names.len(), "名前が重ならない");
    let ops: BTreeSet<&str> = mm_ops::Op::NAMES.iter().copied().collect();
    let extra: BTreeSet<&str> = unique.difference(&ops).copied().collect();
    assert_eq!(extra, BTreeSet::from(NON_OP_TOOLS));
    assert!(ops.is_subset(&unique), "全部の操作がツールになる");

    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(!tool["description"].as_str().unwrap().is_empty(), "{name}");
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{name}");
        let mut found = BTreeSet::new();
        refs(schema, &mut found);
        for r in found {
            let key = r.strip_prefix("#/$defs/").unwrap_or_else(|| {
                panic!("{name}: $defs の外を指す参照 {r}");
            });
            assert!(
                schema["$defs"].get(key).is_some(),
                "{name}: {r} が解決しない"
            );
        }
    }

    let ripple = tools.iter().find(|t| t["name"] == "ripple").unwrap();
    let schema = &ripple["inputSchema"];
    assert!(schema["properties"].get("op").is_none());
    assert!(!schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|k| k == "op"));
    for common in ["label", "base_version", "responds_to"] {
        assert!(schema["properties"].get(common).is_some(), "{common}");
    }
}

#[test]
fn 操作のツールは適用され_actor_は_mcp_で_responds_to_と名前が残る() {
    let r = rig();
    let mut args = marker("mk1", 1);
    args["label"] = json!("マーカー");
    args["responds_to"] = json!("c-1");
    let result = call(&r, "addMarker", args);
    assert!(result.get("isError").is_none(), "{result}");
    let change = text_json(&result);
    assert_eq!(change["version"], 1);
    assert_eq!(change["name"], "マーカー");
    assert_eq!(change["actor"], "mcp");

    let view = r.client.get("/project").unwrap().json().unwrap();
    assert_eq!(
        view["project"]["compositions"][0]["markers"][0]["id"],
        "mk1"
    );

    let log = Store::open_read_only(&r.dir).unwrap().txn_log().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].actor.kind.as_str(), log[0].actor.name.as_str()),
        ("mcp", "claude")
    );
    assert_eq!(log[0].responds_to.as_deref(), Some("c-1"));

    let default_label = call(&r, "removeMarker", json!({"marker": "mk1"}));
    assert_eq!(text_json(&default_label)["name"], "removeMarker");
}

#[test]
fn transaction_は複数の操作を_1_つの取り消しの単位にする() {
    let r = rig();
    let ops = json!([
        {"op": "addMarker", "composition": "main", "marker": marker("a", 1)["marker"]},
        {"op": "addMarker", "composition": "main", "marker": marker("b", 2)["marker"]},
    ]);
    let result = call(
        &r,
        "transaction",
        json!({"name": "対応", "ops": ops, "responds_to": "c-9"}),
    );
    assert!(result.get("isError").is_none(), "{result}");
    assert_eq!(text_json(&result)["txn_id"], 1);

    let undo = text_json(&call(&r, "undo", json!({})));
    assert_eq!(undo["change"]["kind"], "undo");
    let view = text_json(&call(&r, "project", json!({})));
    assert!(view["project"]["compositions"][0]["markers"]
        .as_array()
        .is_none_or(|m| m.is_empty()));
    assert_eq!(
        text_json(&call(&r, "redo", json!({})))["change"]["kind"],
        "redo"
    );
    assert!(text_json(&call(&r, "redo", json!({})))["change"].is_null());

    let log = Store::open_read_only(&r.dir).unwrap().txn_log().unwrap();
    assert_eq!(log[0].responds_to.as_deref(), Some("c-9"));
}

#[test]
fn 戻すものが無ければ_change_は_null() {
    let r = rig();
    let result = call(&r, "undo", json!({}));
    assert!(result.get("isError").is_none());
    assert!(text_json(&result)["change"].is_null());
}

#[test]
fn ロックしたレイヤーに触ると_is_error_で_locked() {
    let r = rig();
    let lock = call(&r, "lockLayer", json!({"layer": "l-footage", "lock": true}));
    assert!(lock.get("isError").is_none(), "{lock}");

    let result = call(
        &r,
        "setParam",
        json!({"target": {"layer": "l-footage", "path": "opacity"},
               "value": {"type": "number", "value": 0.5}}),
    );
    assert_eq!(result["isError"], true);
    assert_eq!(text_json(&result)["error"]["code"], "locked");
    assert_eq!(
        r.client.get("/project").unwrap().json().unwrap()["version"],
        1
    );
}

#[test]
fn 不正な引数と知らないツールは_is_error() {
    let r = rig();
    for (name, args) in [
        ("addMarker", json!({"composition": "main"})),
        ("addMarker", json!({"nothing": 1})),
        (
            "addMarker",
            json!({"label": 3, "composition": "main", "marker": {}}),
        ),
        ("transaction", json!({"name": "x"})),
        ("frame", json!({"t": "abc"})),
        ("nope", json!({})),
    ] {
        let result = call(&r, name, args.clone());
        assert_eq!(result["isError"], true, "{name} {args}");
        assert!(text_json(&result)["error"]["code"].is_string());
    }
    assert_eq!(
        r.client.get("/project").unwrap().json().unwrap()["version"],
        0
    );
}

#[test]
fn frame_は_png_を_base64_の画像で返す() {
    let r = rig();
    let result = call(&r, "frame", json!({"t": "3/2", "width": 320}));
    let image = &result["content"][0];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    assert_eq!(image["data"], "iVBORw0KGgo=");
}

#[test]
fn トークンなしの_mcp_は_401() {
    let r = rig();
    let headers = vec![("Host".to_owned(), format!("127.0.0.1:{}", r.server.port()))];
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    let res = r
        .client
        .send("POST", "/mcp", &headers, Some(body.to_string().as_bytes()))
        .unwrap();
    assert_eq!(res.status, 401);
}

#[test]
fn 通知は_202_でバッチと未知のメソッドは_エラー() {
    let r = rig();
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let res = r.client.post("/mcp", &note).unwrap();
    assert_eq!(res.status, 202);
    assert!(res.body.is_empty());

    let batch = r
        .client
        .post(
            "/mcp",
            &json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}]),
        )
        .unwrap();
    assert_eq!(batch.json().unwrap()["error"]["code"], -32600);

    let unknown = rpc(&r, "resources/list", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    assert_eq!(unknown["id"], 1);

    assert_eq!(rpc(&r, "ping", json!({}))["result"], json!({}));
}
