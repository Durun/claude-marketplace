//! HTTP サーバーの結合テスト。実際の `Store`（一時フォルダーの `project.mmdb`）と実際の待ち受けに、
//! 標準の `TcpStream` のクライアントでつなぐ。

use std::sync::Arc;
use std::time::Duration;

use mm_doc::Project;
use mm_serve::{Client, Core, FrameRequest, FrameSource, Server};
use mm_store::{OpenMode, Store};
use mm_types::Result;
use serde_json::{json, Value};

const NOW: i64 = 1_700_000_000_000;

/// PNG の先頭 8 バイトだけを返す描画の代わり。
struct StubFrames;

impl FrameSource for StubFrames {
    fn frame_png(&self, _project: &Project, request: &FrameRequest) -> Result<Vec<u8>> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(request.time.to_f64().to_string().bytes());
        Ok(bytes)
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
    let dir = tmp.path().join("p");
    let store = Store::create(&dir, &Project::new(), NOW).unwrap();
    let core = Core::new(store, Some(Arc::new(StubFrames)));
    let server = Server::start(core).unwrap();
    let client = Client::new(server.port(), server.token());
    Rig {
        _tmp: tmp,
        dir,
        server,
        client,
    }
}

fn add_sequence(id: &str, cuts: [(&str, i64); 2]) -> Value {
    let cut = |(cid, secs): (&str, i64)| {
        json!({
            "id": cid,
            "source": {"kind": "solid", "version": 1, "params": {}, "inputs": []},
            "in": {"num": 0, "den": 1},
            "out": {"num": secs, "den": 1},
        })
    };
    json!({"op": "addSequence", "sequence": {"id": id, "cuts": [cut(cuts[0]), cut(cuts[1])]}})
}

fn ripple(cut: &str) -> Value {
    json!({"op": "ripple", "cut": cut, "edge": "out", "by": {"num": -2, "den": 5}})
}

fn post_op(c: &Client, op: Value, base: Option<u64>) -> mm_serve::Response {
    let mut body = json!({ "op": op, "name": "テスト" });
    if let Some(b) = base {
        body["base_version"] = json!(b);
    }
    c.post("/ops", &body).unwrap()
}

fn headers_without_token(r: &Rig) -> Vec<(String, String)> {
    vec![("Host".into(), format!("127.0.0.1:{}", r.server.port()))]
}

#[test]
fn 合い言葉が無いか違えば断る() {
    let r = rig();
    let none = r
        .client
        .send("GET", "/project", &headers_without_token(&r), None)
        .unwrap();
    assert_eq!(none.status, 401);

    let mut wrong = headers_without_token(&r);
    wrong.push(("Authorization".into(), "Bearer nope".into()));
    let res = r.client.send("GET", "/project", &wrong, None).unwrap();
    assert_eq!(res.status, 401);

    assert_eq!(r.client.get("/project").unwrap().status, 200);
}

#[test]
fn host_が違えば断る_dns_の再束縛を止める() {
    let r = rig();
    let mut h = r.client.default_headers();
    h[0].1 = "evil.example:80".into();
    let res = r.client.send("GET", "/project", &h, None).unwrap();
    assert_eq!(res.status, 403);
    assert_eq!(res.json().unwrap()["error"]["code"], "forbidden_host");
}

#[test]
fn origin_が別のページなら合い言葉があっても断る() {
    let r = rig();
    let mut h = r.client.default_headers();
    h.push(("Origin".into(), "https://evil.example".into()));
    let res = r.client.send("GET", "/project", &h, None).unwrap();
    assert_eq!(res.status, 403);
    assert_eq!(res.json().unwrap()["error"]["code"], "forbidden_origin");

    let mut same = r.client.default_headers();
    same.push((
        "Origin".into(),
        format!("http://127.0.0.1:{}", r.server.port()),
    ));
    assert_eq!(
        r.client
            .send("GET", "/project", &same, None)
            .unwrap()
            .status,
        200
    );
}

#[test]
fn 合い言葉とポートは_mm_に書かれ_閉じると消える() {
    let r = rig();
    let found = Client::for_project(&r.dir).expect("見つかる");
    assert_eq!(found.port(), r.server.port());
    assert_eq!(found.get("/project").unwrap().status, 200);
    let dir = r.dir.clone();
    drop(r.server);
    assert!(Client::for_project(&dir).is_none());
}

#[test]
fn 操作は_project_mmdb_に書かれ_プロジェクトに見える() {
    let r = rig();
    let res = post_op(&r.client, add_sequence("s1", [("c1", 4), ("c2", 2)]), None);
    assert_eq!(res.status, 200, "{}", res.error_message());
    let change = res.json().unwrap();
    assert_eq!(change["version"], 1);
    assert_eq!(change["kind"], "apply");
    assert_eq!(change["actor"], "cli");
    assert!(change["touched"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "s1"));

    let view = r.client.get("/project").unwrap().json().unwrap();
    assert_eq!(view["version"], 1);
    assert_eq!(view["project"]["sequences"][0]["id"], "s1");

    // 別のプロセスが読み取りだけで開いても、書かれたバージョンが見える。
    let reader = Store::open_read_only(&r.dir).unwrap();
    assert_eq!(reader.mode(), OpenMode::ReadOnly);
    assert_eq!(reader.doc_version(), 1);
}

#[test]
fn 変更は_sse_で届く() {
    let r = rig();
    let mut events = r.client.events().unwrap();
    let hello = events.next_event().unwrap().unwrap();
    assert_eq!(
        (hello.event.as_str(), &hello.data["version"]),
        ("hello", &json!(0))
    );

    post_op(&r.client, add_sequence("s1", [("c1", 4), ("c2", 2)]), None);
    let change = events.next_event().unwrap().unwrap();
    assert_eq!(change.event, "change");
    assert_eq!(change.data["version"], 1);
    assert_eq!(change.data["name"], "テスト");

    r.client.post("/undo", &json!({})).unwrap();
    let undo = events.next_event().unwrap().unwrap();
    assert_eq!(
        (undo.event.as_str(), &undo.data["kind"]),
        ("change", &json!("undo"))
    );
}

#[test]
fn 古い版を元にした操作は_触る所が重ならなければ通り_重なれば断る() {
    let r = rig();
    post_op(&r.client, add_sequence("s1", [("c1", 4), ("c2", 2)]), None);
    post_op(&r.client, add_sequence("s2", [("c3", 4), ("c4", 2)]), None);

    // バージョン 1 を元に s1 を触る。その後の s2 の追加とは重ならない。
    let ok = post_op(&r.client, ripple("c1"), Some(1));
    assert_eq!(ok.status, 200, "{}", ok.error_message());
    assert_eq!(ok.json().unwrap()["version"], 3);

    // 同じバージョン 1 を元に、もう一度 c1 を触る。バージョン 3 の ripple と重なる。
    let conflict = post_op(&r.client, ripple("c1"), Some(1));
    assert_eq!(conflict.status, 409);
    let err = conflict.json().unwrap();
    assert_eq!(err["error"]["code"], "conflict");
    assert!(err["error"]["touches"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "c1"));
    assert_eq!(
        r.client.get("/project").unwrap().json().unwrap()["version"],
        3
    );

    // 今のバージョンより先は別のプロジェクトのデータを指している。
    let future = post_op(&r.client, ripple("c1"), Some(99));
    assert_eq!(future.status, 409);
    assert_eq!(future.json().unwrap()["error"]["code"], "future_base");
}

#[test]
fn 取り消しとやり直しは書かれる() {
    let r = rig();
    post_op(&r.client, add_sequence("s1", [("c1", 4), ("c2", 2)]), None);
    let undo = r.client.post("/undo", &json!({})).unwrap().json().unwrap();
    assert_eq!(undo["change"]["kind"], "undo");
    assert_eq!(undo["change"]["version"], 2);
    let none = r.client.post("/undo", &json!({})).unwrap().json().unwrap();
    assert!(none["change"].is_null(), "取り消すものが無い");
    let redo = r.client.post("/redo", &json!({})).unwrap().json().unwrap();
    assert_eq!(redo["change"]["kind"], "redo");
    assert_eq!(Store::open_read_only(&r.dir).unwrap().doc_version(), 3);
}

#[test]
fn 人の操作経路は_http_から名乗れない() {
    let r = rig();
    let body = json!({"op": add_sequence("s1", [("c1", 4), ("c2", 2)]), "entry": "webview"});
    let res = r.client.post("/ops", &body).unwrap();
    assert_eq!(res.status, 400);
    assert_eq!(
        r.client.get("/project").unwrap().json().unwrap()["version"],
        0
    );
}

#[test]
fn 形の違う本体は_400_で拒否する() {
    let r = rig();
    assert_eq!(
        r.client
            .post("/ops", &json!({"nothing": 1}))
            .unwrap()
            .status,
        400
    );
    let both = json!({"op": ripple("c1"), "txn": {"name": "x", "ops": []}});
    assert_eq!(r.client.post("/ops", &both).unwrap().status, 400);
    let bad_op = r
        .client
        .post("/ops", &json!({"op": {"op": "nope"}}))
        .unwrap();
    assert_eq!(bad_op.status, 400);
    let invalid = post_op(&r.client, ripple("missing"), None);
    assert_eq!(invalid.status, 422);
}

#[test]
fn フレームは_png_で返る() {
    let r = rig();
    let res = r.client.get("/frame?t=3/2").unwrap();
    assert_eq!(res.status, 200, "{}", res.error_message());
    assert_eq!(&res.body[1..4], b"PNG");
    assert!(String::from_utf8_lossy(&res.body).ends_with("1.5"));
    assert_eq!(r.client.get("/frame?t=abc").unwrap().status, 400);
}

#[test]
fn イベントは書かないあいだ来ない() {
    let r = rig();
    let mut events = r.client.events().unwrap();
    events.next_event().unwrap();
    events.set_timeout(Duration::from_millis(200)).unwrap();
    assert!(
        events.next_event().is_err(),
        "書かなければ待ちが時間切れになる"
    );
}
