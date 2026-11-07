//! 提案のエンドポイント（`/proposals`）。形は `docs/design/05-ops.md` の「提案」。
//! 提案を出すのは `POST /ops` と同じ本体で、承認・却下は人の操作経路（`cli`）からだけ受ける。

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::core::{Caller, ServeError};
use crate::http::{blocking, ActorBody, AppState, OpsBody};

/// 提案のルーター。`http::router` が守り（`guard`）の内側に足す。
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/proposals", get(list).post(create))
        .route("/proposals/{id}", get(one))
        .route("/proposals/{id}/approve", post(approve))
        .route("/proposals/{id}/reject", post(reject))
        .route("/proposals/{id}/reopen", post(reopen))
}

/// `ops` のクエリ（`0,2`）を操作の番号の列にする。
pub(crate) fn parse_indices(raw: &str) -> Result<Vec<usize>, ServeError> {
    raw.split(',')
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.trim()
                .parse()
                .map_err(|_| ServeError::BadRequest(format!("操作の番号でない: {s}")))
        })
        .collect()
}

async fn list(State(st): State<Arc<AppState>>) -> Result<Response, ServeError> {
    let core = st.core.clone();
    let proposals = blocking(move || core.proposals()).await?;
    Ok(Json(json!({ "proposals": proposals })).into_response())
}

async fn one(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Result<Response, ServeError> {
    let core = st.core.clone();
    let view = blocking(move || core.proposal(id)).await?;
    Ok(Json(view).into_response())
}

/// `POST /ops` と同じ本体（`txn` か `op`、`entry`、`actor`、`responds_to`）を、適用せずに保留する。
async fn create(State(st): State<Arc<AppState>>, body: Bytes) -> Result<Response, ServeError> {
    let mut parsed: OpsBody = serde_json::from_slice(&body)
        .map_err(|e| ServeError::BadRequest(format!("本体の形が違う: {e}")))?;
    let txn = parsed.transaction()?;
    let caller = parsed.caller()?;
    let responds_to = parsed.responds_to.take();
    let core = st.core.clone();
    let view = blocking(move || core.propose(txn, &caller, responds_to)).await?;
    Ok(Json(view).into_response())
}

/// 承認・却下・保留に戻すの本体。どれも任意。`ops` は承認する操作の番号（省くと全部）。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideBody {
    ops: Option<Vec<usize>>,
    entry: Option<String>,
    actor: Option<ActorBody>,
}

fn decide_body(body: &Bytes) -> Result<(Option<Vec<usize>>, Caller), ServeError> {
    let parsed: DecideBody = if body.is_empty() {
        DecideBody::default()
    } else {
        serde_json::from_slice(body)
            .map_err(|e| ServeError::BadRequest(format!("本体の形が違う: {e}")))?
    };
    let caller = OpsBody {
        txn: None,
        op: None,
        name: None,
        base_version: None,
        entry: parsed.entry,
        actor: parsed.actor,
        responds_to: None,
    }
    .caller()?;
    Ok((parsed.ops, caller))
}

/// 却下と保留に戻すは提案の全体に掛かるので、操作の番号は受けない。
fn decide_whole(body: &Bytes) -> Result<Caller, ServeError> {
    match decide_body(body)? {
        (None, caller) => Ok(caller),
        (Some(_), _) => Err(ServeError::BadRequest(
            "ops は承認（approve）にだけ渡せる".into(),
        )),
    }
}

async fn approve(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Result<Response, ServeError> {
    let (ops, caller) = decide_body(&body)?;
    let core = st.core.clone();
    let approval = blocking(move || core.approve(id, ops.as_deref(), &caller)).await?;
    Ok(Json(approval).into_response())
}

async fn reject(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Result<Response, ServeError> {
    let caller = decide_whole(&body)?;
    let core = st.core.clone();
    let view = blocking(move || core.reject(id, &caller)).await?;
    Ok(Json(view).into_response())
}

async fn reopen(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Result<Response, ServeError> {
    let caller = decide_whole(&body)?;
    let core = st.core.clone();
    let view = blocking(move || core.reopen(id, &caller)).await?;
    Ok(Json(view).into_response())
}
