//! MCP（Streamable HTTP、ステートレス、JSON 応答）の `POST /mcp`。
//!
//! ツールの定義は [`crate::mcp_tools`]。ここは JSON-RPC の振り分けと、ツールの呼び出しを
//! `Core` の操作に結ぶ。認証は `router` の `guard` が済ませている。

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mm_ops::{Op, Transaction};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::core::{Caller, ServeError};
use crate::frames::{parse_time, FrameRequest};
use crate::http::{blocking, error_body, AppState};
use crate::mcp_tools::{tools, COMMON_ARGS};

/// 応じられる MCP の仕様の日付。先頭が最新で、要求が無いか知らない日付のときに返す（仕様の版の交渉）。
const SUPPORTED_PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "動画編集アプリ movie-maker を操作する。操作ごとのツール（ripple、addLayer など）は 1 回で 1 つの取り消しの単位になる。\
コメントへの対応のように複数の操作を 1 つの単位にしたいときは transaction を使い、responds_to にコメントの ID を渡す。\
frame で仕上がりを見て確かめ、間違えたら undo で戻す。ロックの付いたレイヤーには触れない。\
人に見てから入れてほしい編集は propose で提案として保留する（承認と却下は人がアプリでする）。proposals で状態を読み、frame の proposal で提案を適用したフレームを見る。";

/// `POST /mcp`。通知（`id` が無い）は 202 で本体なし、それ以外は JSON-RPC の応答を JSON で返す。
pub(crate) async fn post_mcp(State(st): State<Arc<AppState>>, body: Bytes) -> Response {
    let message: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return rpc_error(Value::Null, PARSE_ERROR, &format!("JSON が読めない: {e}")),
    };
    let Some(obj) = message.as_object() else {
        return rpc_error(
            Value::Null,
            INVALID_REQUEST,
            "バッチは受けない。要求を 1 つずつ送る",
        );
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // クライアントからの応答（result か error）は受け取るだけで返さない。
        if obj.contains_key("result") || obj.contains_key("error") {
            return StatusCode::ACCEPTED.into_response();
        }
        return rpc_error(Value::Null, INVALID_REQUEST, "method が無い");
    };
    let Some(id) = obj.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => rpc_result(id, initialize(&params)),
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tools().list })),
        "tools/call" => match call_params(&params) {
            Ok((name, args)) => rpc_result(id, call_tool(&st, name, args).await),
            Err(msg) => rpc_error(id, INVALID_PARAMS, &msg),
        },
        other => rpc_error(id, METHOD_NOT_FOUND, &format!("{other} は未対応")),
    }
}

fn rpc_result(id: Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Response {
    let status = if id.is_null() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    let body = json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } });
    (status, Json(body)).into_response()
}

fn initialize(params: &Value) -> Value {
    let version = params["protocolVersion"]
        .as_str()
        .filter(|v| SUPPORTED_PROTOCOLS.contains(v))
        .unwrap_or(SUPPORTED_PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "movie-maker", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn call_params(params: &Value) -> Result<(&str, Map<String, Value>), String> {
    let name = params["name"]
        .as_str()
        .ok_or("params.name が文字列でない")?;
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => return Err("params.arguments がオブジェクトでない".into()),
    };
    Ok((name, args))
}

/// ツールの成否はどちらも `result` に入れる。失敗は `isError` と、HTTP の失敗と同じ形の JSON。
async fn call_tool(st: &Arc<AppState>, name: &str, args: Map<String, Value>) -> Value {
    match run_tool(st, name, args).await {
        Ok(content) => json!({ "content": content }),
        Err(err) => {
            let (_, body) = error_body(&err);
            json!({ "isError": true, "content": [text(&body)] })
        }
    }
}

fn text(value: &impl serde::Serialize) -> Value {
    json!({ "type": "text", "text": serde_json::to_string(value).unwrap_or_default() })
}

async fn run_tool(
    st: &Arc<AppState>,
    name: &str,
    args: Map<String, Value>,
) -> Result<Vec<Value>, ServeError> {
    let core = st.core.clone();
    let caller = Caller::mcp();
    match name {
        "transaction" | "propose" => {
            let a: TransactionArgs = parse_args(args)?;
            let mut txn = Transaction::new(a.name, a.ops);
            txn.base_version = a.base_version;
            let result = if name == "propose" {
                text(&blocking(move || core.propose(txn, &caller, a.responds_to)).await?)
            } else {
                text(&blocking(move || core.apply(txn, &caller, a.responds_to)).await?)
            };
            Ok(vec![result])
        }
        "proposals" => {
            let proposals = blocking(move || core.proposals()).await?;
            Ok(vec![text(&json!({ "proposals": proposals }))])
        }
        "undo" => {
            let change = blocking(move || core.undo(&caller)).await?;
            Ok(vec![text(&json!({ "change": change }))])
        }
        "redo" => {
            let change = blocking(move || core.redo(&caller)).await?;
            Ok(vec![text(&json!({ "change": change }))])
        }
        "project" => {
            let view = blocking(move || core.project()).await?;
            Ok(vec![text(&view)])
        }
        "frame" => frame(core, args).await,
        op if tools().op_names.contains(op) => {
            let (txn, responds_to) = op_transaction(op, args)?;
            let change = blocking(move || core.apply(txn, &caller, responds_to)).await?;
            Ok(vec![text(&change)])
        }
        other => Err(ServeError::BadRequest(format!(
            "{other} というツールは無い（tools/list で確かめる）"
        ))),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionArgs {
    name: String,
    ops: Vec<Op>,
    base_version: Option<u64>,
    responds_to: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameArgs {
    t: String,
    comp: Option<String>,
    width: Option<u32>,
    /// 提案の ID。あれば提案を適用したフレームを描く（書かない）。
    proposal: Option<i64>,
    /// 提案のうち適用する操作の番号。省くと全部。
    ops: Option<Vec<usize>>,
}

/// `frame`。`GET /frame` と同じく、`proposal` があれば提案を適用したフレームを描く。
async fn frame(
    core: Arc<crate::core::Core>,
    args: Map<String, Value>,
) -> Result<Vec<Value>, ServeError> {
    let a: FrameArgs = parse_args(args)?;
    if a.ops.is_some() && a.proposal.is_none() {
        return Err(ServeError::BadRequest(
            "ops は proposal と一緒に渡す".into(),
        ));
    }
    let request = FrameRequest {
        composition: a.comp,
        time: parse_time(&a.t)?,
        width: a.width,
    };
    let png = blocking(move || match a.proposal {
        Some(id) => core.preview_frame_png(id, a.ops.as_deref(), &request),
        None => core.frame_png(&request),
    })
    .await?;
    Ok(vec![json!({
        "type": "image",
        "data": base64(&png),
        "mimeType": "image/png",
    })])
}

fn parse_args<T: serde::de::DeserializeOwned>(args: Map<String, Value>) -> Result<T, ServeError> {
    serde_json::from_value(Value::Object(args))
        .map_err(|e| ServeError::BadRequest(format!("引数の形が違う: {e}")))
}

/// 共通の引数（`label`、`base_version`、`responds_to`）を抜き、残りに `op` を足して 1 操作のトランザクションにする。
fn op_transaction(
    name: &str,
    mut args: Map<String, Value>,
) -> Result<(Transaction, Option<String>), ServeError> {
    let [label, base, responds_to] = COMMON_ARGS.map(|k| args.remove(k));
    let owned_str = |v: &Value| v.as_str().map(str::to_owned);
    let label = optional(label, owned_str, "label", "文字列")?;
    let responds_to = optional(responds_to, owned_str, "responds_to", "文字列")?;
    let base = optional(base, Value::as_u64, "base_version", "0 以上の整数")?;
    args.insert("op".into(), json!(name));
    let op: Op = parse_args(args)?;
    let mut txn = Transaction::new(label.unwrap_or_else(|| name.to_owned()), vec![op]);
    txn.base_version = base;
    Ok((txn, responds_to))
}

fn optional<T>(
    value: Option<Value>,
    get: impl Fn(&Value) -> Option<T>,
    key: &str,
    expected: &str,
) -> Result<Option<T>, ServeError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => get(&v)
            .map(Some)
            .ok_or_else(|| ServeError::BadRequest(format!("{key} は{expected}"))),
    }
}

/// 標準の base64（パディングあり）。画像 1 枚の返却のためだけなので依存は足さない。
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_は_rfc4648_の例と一致する() {
        for (src, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(src.as_bytes()), want);
        }
    }
}
