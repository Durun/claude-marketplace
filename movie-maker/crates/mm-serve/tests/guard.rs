//! HTTP サーバーの拒否の経路と、名乗れる範囲の結合テスト。
//! 拒否した要求は本体を読まず、プロジェクトのデータを変えない。

use std::path::Path;

use mm_doc::Project;
use mm_serve::{Client, Core, Server};
use mm_store::Store;
use serde_json::{json, Value};

const NOW: i64 = 1_700_000_000_000;
/// `BODY_LIMIT`（8 MB）を超える本体の大きさ。
const OVER_LIMIT: usize = 9 * 1024 * 1024;

struct Rig {
    _tmp: tempfile::TempDir,
    server: Server,
    client: Client,
}

/// 段階 0 のフィクスチャ（ロックを付けられるレイヤー `l-text` がある）を書き手で開いた HTTP サーバー。
fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/stage0/project.json");
    let project: Project = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let store = Store::create(&tmp.path().join("p"), &project, NOW).unwrap();
    let server = Server::start(Core::new(store, None)).unwrap();
    let client = Client::new(server.port(), server.token());
    Rig {
        _tmp: tmp,
        server,
        client,
    }
}

fn version(r: &Rig) -> i64 {
    r.client.get("/project").unwrap().json().unwrap()["version"]
        .as_i64()
        .unwrap()
}

fn opacity(value: f64) -> Value {
    json!({
        "op": {"op": "setParam", "target": {"layer": "l-footage", "path": "opacity"},
               "value": {"type": "number", "value": value}},
        "name": "テスト"
    })
}

fn post(r: &Rig, headers: &[(String, String)], body: &Value) -> mm_serve::Response {
    r.client
        .send("POST", "/ops", headers, Some(body.to_string().as_bytes()))
        .unwrap()
}

fn with(r: &Rig, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut h = r.client.default_headers();
    h.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    h
}

#[test]
fn ops_は_host_origin_合い言葉が違えば適用されない() {
    let r = rig();
    let body = opacity(0.5);
    let port = r.server.port();

    let mut bad_host = r.client.default_headers();
    bad_host[0].1 = "evil.example:80".into();
    assert_eq!(post(&r, &bad_host, &body).status, 403);

    let evil_origin = with(&r, &[("Origin", "https://evil.example")]);
    assert_eq!(post(&r, &evil_origin, &body).status, 403);

    // `Origin: null` はサンドボックスの iframe やファイルのページが付ける。
    let null_origin = with(&r, &[("Origin", "null")]);
    assert_eq!(post(&r, &null_origin, &body).status, 403);

    let no_host = vec![(
        "Authorization".to_owned(),
        format!("Bearer {}", r.server.token()),
    )];
    assert_eq!(post(&r, &no_host, &body).status, 403);

    let no_token = vec![("Host".to_owned(), format!("127.0.0.1:{port}"))];
    assert_eq!(post(&r, &no_token, &body).status, 401);

    let wrong = vec![
        ("Host".to_owned(), format!("127.0.0.1:{port}")),
        ("Authorization".to_owned(), "Bearer nope".to_owned()),
    ];
    assert_eq!(post(&r, &wrong, &body).status, 401);

    assert_eq!(version(&r), 0, "どれも適用されない");
    assert_eq!(post(&r, &r.client.default_headers(), &body).status, 200);
    assert_eq!(version(&r), 1);
}

#[test]
fn 本体が上限を超えれば_413_で適用されない() {
    let r = rig();
    let big = json!({ "op": opacity(0.5)["op"], "name": "x".repeat(OVER_LIMIT) });
    let res = post(&r, &r.client.default_headers(), &big);
    assert_eq!(res.status, 413);
    assert_eq!(version(&r), 0);
}

#[test]
fn ロックの付いた所は_http_から触れず_403_locked() {
    let r = rig();
    let lock = json!({"op": {"op": "lockLayer", "layer": "l-footage", "lock": true}});
    assert_eq!(post(&r, &r.client.default_headers(), &lock).status, 200);

    for entry in ["cli", "mcp"] {
        let mut body = opacity(0.5);
        body["entry"] = json!(entry);
        let res = post(&r, &r.client.default_headers(), &body);
        assert_eq!(res.status, 403, "{entry}: {}", res.error_message());
        assert_eq!(res.json().unwrap()["error"]["code"], "locked");
    }
    assert_eq!(version(&r), 1, "ロックの操作だけが適用されている");
}

#[test]
fn actor_の_kind_は操作経路で決まり_名乗れない() {
    let r = rig();
    let h = r.client.default_headers();

    let mut spoof = opacity(0.5);
    spoof["actor"] = json!({"kind": "gui", "name": "human"});
    assert_eq!(post(&r, &h, &spoof).status, 400, "kind は受けない");
    assert_eq!(version(&r), 0);

    let mut named = opacity(0.5);
    named["actor"] = json!({"name": "my-script"});
    let cli = post(&r, &h, &named).json().unwrap();
    assert_eq!(cli["actor"], "cli");

    let mut mcp = opacity(0.6);
    mcp["entry"] = json!("mcp");
    let mcp = post(&r, &h, &mcp).json().unwrap();
    assert_eq!(mcp["actor"], "mcp");

    let mut mcp_named = opacity(0.7);
    mcp_named["entry"] = json!("mcp");
    mcp_named["actor"] = json!({"name": "claude-code"});
    assert_eq!(post(&r, &h, &mcp_named).json().unwrap()["actor"], "mcp");
}

#[test]
fn 文字起こしは素材に入り_素材が無ければ断る() {
    let r = rig();
    let project = r.client.get("/project").unwrap().json().unwrap();
    let asset = &project["project"]["assets"][0];
    let word = |text: &str, s: i64| json!({"segment": 0, "start": {"num": s, "den": 1}, "end": {"num": s + 1, "den": 1}, "text": text});
    let body = |asset_id: &str| {
        json!({
            "asset": asset_id, "content_hash": asset["content_hash"],
            "producer": "test", "language": "ja", "words": [word("こんにちは", 0), word("世界", 1)],
        })
    };
    let ok = r
        .client
        .post("/transcripts", &body(asset["id"].as_str().unwrap()))
        .unwrap();
    assert_eq!(ok.status, 200, "{}", ok.error_message());
    assert_eq!(ok.json().unwrap()["words"], 2);

    let missing = r.client.post("/transcripts", &body("nope")).unwrap();
    assert_eq!(missing.status, 400);

    let no_token = vec![("Host".to_owned(), format!("127.0.0.1:{}", r.server.port()))];
    let res = r
        .client
        .send(
            "POST",
            "/transcripts",
            &no_token,
            Some(body("x").to_string().as_bytes()),
        )
        .unwrap();
    assert_eq!(res.status, 401);
}

#[test]
fn 取り消しは人の操作を戻したと返す() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::create(&tmp.path().join("p"), &Project::new(), NOW).unwrap();
    let core = Core::new(store, None);
    let server = Server::start(core.clone()).unwrap();
    let client = Client::new(server.port(), server.token());
    let op: mm_ops::Transaction = serde_json::from_value(json!({
        "name": "人がした操作",
        "ops": [{"op": "addSequence", "sequence": {"id": "s1", "cuts": []}}],
    }))
    .unwrap();
    core.apply(op, &mm_serve::Caller::webview(), None).unwrap();

    let undo = client.post("/undo", &json!({})).unwrap().json().unwrap();
    let of = &undo["change"]["of"];
    assert_eq!(
        (of["actor"].as_str(), of["name"].as_str()),
        (Some("gui"), Some("人がした操作"))
    );
    assert_eq!(undo["change"]["actor"], "cli", "取り消した側は cli");
    let redo = client.post("/redo", &json!({})).unwrap().json().unwrap();
    assert_eq!(redo["change"]["of"]["actor"], "gui");
}

#[test]
fn クラッシュで残った古いポートと合い言葉は信じない() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let writer = Store::create(&dir, &Project::new(), NOW).unwrap();
    writer.publish_token("stale-token").unwrap();
    writer.publish_http_port(1).unwrap();
    let info_path = dir.join(".mm").join("lock.info");
    let token_path = Store::writer_token_file(&dir).unwrap();
    let (info, token) = (
        std::fs::read(&info_path).unwrap(),
        std::fs::read(&token_path).unwrap(),
    );
    assert!(
        Client::for_project(&dir).is_some(),
        "書き手が生きている間は見つかる"
    );

    // 書き手がクラッシュして、ロックだけが解けた状態を作る。
    drop(writer);
    std::fs::write(&info_path, info).unwrap();
    std::fs::write(&token_path, token).unwrap();
    assert_eq!(
        Store::read_writer_token(&dir).as_deref(),
        Some("stale-token")
    );
    assert!(Client::for_project(&dir).is_none());
    std::fs::remove_file(token_path).unwrap();
}
