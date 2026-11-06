//! `POST /transcripts`: 素材の文字起こしを DB の表に入れる。
//!
//! 文字起こしはプロジェクトのデータの操作ではなく DB の表なので `/ops` に載せられない。
//! アプリが書き手のとき、`mm ingest` が付属ファイルの本文をここへ送る。

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mm_store::{TranscriptInput, WordInput};
use mm_types::{AssetId, RationalTime};
use serde::Deserialize;
use serde_json::json;

use crate::core::ServeError;
use crate::http::{blocking, AppState};

/// 本体。`words` の時刻は操作と同じ有理数の JSON。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranscriptBody {
    asset: AssetId,
    content_hash: String,
    producer: String,
    language: String,
    words: Vec<WordBody>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WordBody {
    segment: u32,
    start: RationalTime,
    end: RationalTime,
    text: String,
}

pub(crate) async fn post_transcripts(
    State(st): State<Arc<AppState>>,
    body: Bytes,
) -> Result<Response, ServeError> {
    let parsed: TranscriptBody = serde_json::from_slice(&body)
        .map_err(|e| ServeError::BadRequest(format!("本体の形が違う: {e}")))?;
    let input = TranscriptInput {
        producer: parsed.producer,
        language: parsed.language,
        words: parsed
            .words
            .into_iter()
            .map(|w| WordInput {
                segment: w.segment,
                start: w.start,
                end: w.end,
                text: w.text,
            })
            .collect(),
    };
    let core = st.core.clone();
    let (asset, hash) = (parsed.asset, parsed.content_hash);
    let words = blocking(move || core.put_transcript(&asset, &hash, &input)).await?;
    Ok(Json(json!({ "words": words })).into_response())
}
