//! HTTP サーバー（axum）。パスと JSON の形は `docs/design/05-ops.md` の「HTTP サーバー」。
//!
//! すべての要求は、先に `guard` で `Host`、`Origin`、トークンを確かめる。ブラウザーのページは
//! DNS の再束縛や別のオリジンからの要求で、ローカルの待ち受けに触れる。それをここで止める。

use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use mm_ops::{Entry, Op, OpError, Transaction};
use mm_store::Actor;
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::core::{Caller, Core, ServeError};
use crate::frames::{parse_time, FrameRequest};

/// 要求の本体の上限。`addAsset` の束や長いトランザクションが収まる大きさ。
const BODY_LIMIT: usize = 8 * 1024 * 1024;

/// HTTP サーバーのハンドラーが共有するもの。
pub(crate) struct AppState {
    pub core: Arc<Core>,
    pub token: String,
    /// `Host` に許す値（`127.0.0.1:<port>`、`localhost:<port>`）。
    pub hosts: Vec<String>,
}

/// ルーター。
pub(crate) fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ops", post(post_ops))
        .route("/transcripts", post(crate::transcripts::post_transcripts))
        .route("/undo", post(post_undo))
        .route("/redo", post(post_redo))
        .route("/project", get(get_project))
        .route("/frame", get(get_frame))
        .route("/events", get(get_events))
        .route("/mcp", post(crate::mcp::post_mcp))
        .merge(crate::proposals_http::routes())
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// `Host`、`Origin`、トークンを確かめる。どれかが違えば本体を読まずに断る。
async fn guard(State(st): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    if !header_in(headers, header::HOST, &st.hosts) {
        return error_response(StatusCode::FORBIDDEN, "forbidden_host", "Host が違う");
    }
    // ブラウザーのページからの要求は Origin を付ける。この HTTP サーバーを使う画面は同じオリジンのものだけ。
    if let Some(origin) = headers.get(header::ORIGIN) {
        let allowed = origin
            .to_str()
            .ok()
            .and_then(|o| o.strip_prefix("http://"))
            .is_some_and(|o| st.hosts.iter().any(|h| h == o));
        if !allowed {
            return error_response(StatusCode::FORBIDDEN, "forbidden_origin", "Origin が違う");
        }
    }
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if !given.is_some_and(|g| constant_time_eq(g.as_bytes(), st.token.as_bytes())) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "トークンが無いか違う（Authorization: Bearer で渡す）",
        );
    }
    next.run(req).await
}

fn header_in(headers: &HeaderMap, name: header::HeaderName, allowed: &[String]) -> bool {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| allowed.iter().any(|a| a.eq_ignore_ascii_case(v)))
}

/// トークンの比較は、どこで違ったかを時間に出さない。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

impl IntoResponse for ServeError {
    fn into_response(self) -> Response {
        let (status, body) = error_body(&self);
        (status, Json(body)).into_response()
    }
}

/// HTTP の失敗の応答と同じ `{"error": {"code","message", ...}}`。MCP の `isError` の本文にも使う。
pub(crate) fn error_body(err: &ServeError) -> (StatusCode, serde_json::Value) {
    let message = err.to_string();
    let (status, code, extra) = match err {
        ServeError::Op(OpError::Conflict { base, touches }) => (
            StatusCode::CONFLICT,
            "conflict",
            json!({ "base": base, "touches": touches.to_vec() }),
        ),
        ServeError::Op(OpError::FutureBase { base, current }) => (
            StatusCode::CONFLICT,
            "future_base",
            json!({ "base": base, "current": current }),
        ),
        ServeError::Op(OpError::Locked(touches)) => (
            StatusCode::FORBIDDEN,
            "locked",
            json!({ "touches": touches.to_vec() }),
        ),
        ServeError::Op(OpError::Doc(_)) => (StatusCode::UNPROCESSABLE_ENTITY, "invalid", json!({})),
        ServeError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request", json!({})),
        ServeError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable", json!({})),
        ServeError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal", json!({})),
        ServeError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden", json!({})),
    };
    let mut body = json!({ "code": code, "message": message });
    if let (Some(obj), Some(more)) = (body.as_object_mut(), extra.as_object()) {
        obj.extend(more.clone());
    }
    (status, json!({ "error": body }))
}

/// `POST /ops` の本体。`txn` か `op` のどちらか 1 つ。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpsBody {
    /// トランザクション（`name`、`base_version`、`ops`）。
    pub(crate) txn: Option<Transaction>,
    /// 操作 1 つ。`name` と `base_version` でトランザクションにする。
    pub(crate) op: Option<Op>,
    pub(crate) name: Option<String>,
    pub(crate) base_version: Option<u64>,
    /// 操作経路。`cli`（既定）か `mcp`。ロックの付いた所に触る操作は、どちらでも拒否する。
    pub(crate) entry: Option<String>,
    pub(crate) actor: Option<ActorBody>,
    /// 対応しているコメントの ID。
    pub(crate) responds_to: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActorBody {
    /// 呼び出し側の名前だけ。種類（`cli` か `mcp`）は操作経路で決まり、名乗れない。
    name: String,
}

impl OpsBody {
    pub(crate) fn transaction(&mut self) -> Result<Transaction, ServeError> {
        match (self.txn.take(), self.op.take()) {
            (Some(txn), None) => {
                if self.name.is_some() || self.base_version.is_some() {
                    return Err(ServeError::BadRequest(
                        "txn と一緒に name と base_version は渡せない（txn の中に書く）".into(),
                    ));
                }
                Ok(txn)
            }
            (None, Some(op)) => {
                let mut txn =
                    Transaction::new(self.name.take().unwrap_or_else(|| "op".into()), vec![op]);
                txn.base_version = self.base_version;
                Ok(txn)
            }
            _ => Err(ServeError::BadRequest(
                "txn か op のどちらか 1 つを渡す".into(),
            )),
        }
    }

    pub(crate) fn caller(&mut self) -> Result<Caller, ServeError> {
        let (entry, default_actor) = match self.entry.as_deref().unwrap_or("cli") {
            "cli" => (Entry::Cli, ("cli", "mm")),
            "mcp" => (Entry::Mcp, ("mcp", "claude")),
            other => {
                return Err(ServeError::BadRequest(format!(
                    "entry は cli か mcp（{other} は HTTP からは使えない）"
                )))
            }
        };
        let name = self
            .actor
            .take()
            .map_or_else(|| default_actor.1.to_owned(), |a| a.name);
        let actor = Actor {
            kind: default_actor.0.into(),
            name,
        };
        Ok(Caller { entry, actor })
    }
}

async fn post_ops(State(st): State<Arc<AppState>>, body: Bytes) -> Result<Response, ServeError> {
    let mut parsed: OpsBody = serde_json::from_slice(&body)
        .map_err(|e| ServeError::BadRequest(format!("本体の形が違う: {e}")))?;
    let txn = parsed.transaction()?;
    let caller = parsed.caller()?;
    let responds_to = parsed.responds_to.take();
    let core = st.core.clone();
    let change = blocking(move || core.apply(txn, &caller, responds_to)).await?;
    Ok(Json(change).into_response())
}

/// 取り消しとやり直しの本体は任意。`{ "entry": "cli" }` のように操作経路を決められる。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryBody {
    pub(crate) entry: Option<String>,
    pub(crate) actor: Option<ActorBody>,
}

fn history_caller(body: &Bytes) -> Result<Caller, ServeError> {
    let parsed: HistoryBody = if body.is_empty() {
        HistoryBody::default()
    } else {
        serde_json::from_slice(body)
            .map_err(|e| ServeError::BadRequest(format!("本体の形が違う: {e}")))?
    };
    OpsBody {
        txn: None,
        op: None,
        name: None,
        base_version: None,
        entry: parsed.entry,
        actor: parsed.actor,
        responds_to: None,
    }
    .caller()
}

async fn post_undo(State(st): State<Arc<AppState>>, body: Bytes) -> Result<Response, ServeError> {
    let caller = history_caller(&body)?;
    let core = st.core.clone();
    let change = blocking(move || core.undo(&caller)).await?;
    Ok(history_response(change))
}

async fn post_redo(State(st): State<Arc<AppState>>, body: Bytes) -> Result<Response, ServeError> {
    let caller = history_caller(&body)?;
    let core = st.core.clone();
    let change = blocking(move || core.redo(&caller)).await?;
    Ok(history_response(change))
}

/// 取り消すものが無いときは `{"change": null}`（エラーにしない）。
fn history_response(change: Option<crate::core::Change>) -> Response {
    Json(json!({ "change": change })).into_response()
}

async fn get_project(State(st): State<Arc<AppState>>) -> Result<Response, ServeError> {
    let core = st.core.clone();
    let view = blocking(move || core.project()).await?;
    Ok(Json(view).into_response())
}

#[derive(Debug, Deserialize)]
struct FrameQuery {
    t: String,
    comp: Option<String>,
    width: Option<u32>,
    /// 提案の ID。あれば提案を適用したフレームを描く（書かない）。
    proposal: Option<i64>,
    /// 提案のうち適用する操作の番号（`0,2`）。省くと全部。
    ops: Option<String>,
}

async fn get_frame(
    State(st): State<Arc<AppState>>,
    Query(q): Query<FrameQuery>,
) -> Result<Response, ServeError> {
    let request = FrameRequest {
        composition: q.comp,
        time: parse_time(&q.t)?,
        width: q.width,
    };
    let selection = q
        .ops
        .as_deref()
        .map(crate::proposals_http::parse_indices)
        .transpose()?;
    let core = st.core.clone();
    let png = blocking(move || match q.proposal {
        Some(id) => core.preview_frame_png(id, selection.as_deref(), &request),
        None => core.frame_png(&request),
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, "image/png")], png).into_response())
}

/// 変更の通知（SSE）。最初に `hello`（今のバージョン）、以後は書くたびに `change`。
/// 取りこぼしたら `resync`（今のバージョン）を送るので、`GET /project` で読み直す。
async fn get_events(
    State(st): State<Arc<AppState>>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ServeError> {
    let rx = st.core.subscribe();
    let version = st.core.version()?;
    let core = st.core.clone();
    let hello = Event::default()
        .event("hello")
        .json_data(json!({ "version": version }))
        .map_err(|e| ServeError::Internal(e.to_string()));
    let changes = BroadcastStream::new(rx).map(move |item| {
        let event = match item {
            Ok(change) => Event::default().event("change").json_data(&change),
            Err(BroadcastStreamRecvError::Lagged(_)) => {
                let version = core.version().unwrap_or(-1);
                Event::default()
                    .event("resync")
                    .json_data(json!({ "version": version }))
            }
        };
        Ok(event.unwrap_or_else(|e| Event::default().event("error").data(e.to_string())))
    });
    // 提案の通知は取りこぼしても `GET /proposals` で読み直せるので、取りこぼしは送らない。
    let proposals = BroadcastStream::new(st.core.subscribe_proposals()).filter_map(|item| {
        let event = Event::default().event("proposal").json_data(item.ok()?);
        Some(Ok(event.unwrap_or_else(|e| {
            Event::default().event("error").data(e.to_string())
        })))
    });
    let stream = tokio_stream::iter([Ok(hello?)]).chain(changes.merge(proposals));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// SQLite と描画は同期の処理なので、非同期の実行環境を止めない別のスレッドで行う。
pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ServeError> + Send + 'static,
) -> Result<T, ServeError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ServeError::Internal(format!("処理が途中で終わった: {e}")))?
}
