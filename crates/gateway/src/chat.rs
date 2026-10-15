//! `POST /v1/chat/completions` — OpenAI-compatible proxy with metering.
//!
//! M1 subset of the SPEC §5 lifecycle: auth (step 2) → forward to the single
//! OpenAI upstream → stream passthrough with token extraction (step 5) →
//! append `usage_events` (step 6). Quota/rate checks (step 3), key selection
//! (step 4) and health signals (step 7) are M2+.

use std::convert::Infallible;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::db::{self, UsageEvent};
use crate::error::ApiError;
use crate::keys::AuthedKey;
use crate::sse::{SseMeter, UsageOutcome};
use crate::state::AppState;

/// Non-streaming calls may buffer; streaming calls must never hit it.
const NON_STREAM_TIMEOUT: Duration = Duration::from_secs(120);

pub async fn chat_completions(
    State(state): State<AppState>,
    auth: AuthedKey,
    body: Bytes,
) -> Result<Response, ApiError> {
    let started = Instant::now();
    let mut request_body: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::BadRequest(format!("invalid JSON body: {e}")))?;
    let requested_model = request_body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let is_stream = request_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Ask upstream for stream usage when the client didn't; remember whether
    // the resulting usage chunk must then be stripped downstream.
    let mut strip_injected_usage = false;
    if is_stream {
        let client_asked = request_body
            .pointer("/stream_options/include_usage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !client_asked {
            request_body["stream_options"]["include_usage"] = Value::Bool(true);
            strip_injected_usage = true;
        }
    }

    let url = format!("{}/chat/completions", state.config.openai_base_url);
    let mut upstream = state
        .http
        .post(&url)
        .bearer_auth(&state.config.openai_api_key)
        .json(&request_body);
    if !is_stream {
        upstream = upstream.timeout(NON_STREAM_TIMEOUT);
    }
    let response = upstream.send().await.map_err(|e| {
        tracing::warn!("upstream request failed: {e:#}");
        ApiError::BadGateway("upstream request failed".to_string())
    })?;

    let status = response.status();
    if !is_stream || !status.is_success() {
        // Buffered path: non-streaming responses and upstream errors.
        buffered_response(state, auth, response, requested_model, started).await
    } else {
        // Streaming success: SSE passthrough with metering.
        streaming_response(
            state,
            auth,
            response,
            requested_model,
            started,
            strip_injected_usage,
        )
    }
}

/// Forwards a fully-buffered upstream response (non-streaming or any error)
/// and records its usage event.
async fn buffered_response(
    state: AppState,
    auth: AuthedKey,
    response: reqwest::Response,
    requested_model: String,
    started: Instant,
) -> Result<Response, ApiError> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = response
        .bytes()
        .await
        .map_err(|e| ApiError::BadGateway(format!("upstream body read failed: {e}")))?;

    let outcome = parse_nonstream_usage(&bytes);
    let model = outcome.model.clone().unwrap_or(requested_model);
    let event = UsageEvent {
        id: db::new_id(),
        ts: db::now_ms(),
        pool_key_id: auth.id,
        user_id: auth.user_id,
        model,
        prompt_tokens: outcome.prompt_tokens,
        completion_tokens: outcome.completion_tokens,
        cost_micros: 0, // pricing table lands M2+ (D23)
        latency_ms: started.elapsed().as_millis().min(i64::MAX as u128) as i64,
        status: status.as_u16().to_string(),
    };
    if let Err(err) = db::insert_usage_event(&state.db, &event).await {
        // Metering is the point of M1 — fail loudly rather than drop rows.
        return Err(ApiError::Internal(anyhow::anyhow!(
            "failed to record usage event: {err}"
        )));
    }

    let mut response = Response::builder().status(StatusCode::from_u16(status.as_u16()).unwrap());
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    } else {
        response = response.header(header::CONTENT_TYPE, "application/json");
    }
    response
        .body(Body::from(bytes))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("response build failed: {e}")))
}

/// Success path for streaming: spawns the meter task, returns the passthrough
/// body immediately. The usage row is written when the upstream stream ends.
fn streaming_response(
    state: AppState,
    auth: AuthedKey,
    response: reqwest::Response,
    requested_model: String,
    started: Instant,
    strip_injected_usage: bool,
) -> Result<Response, ApiError> {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| "text/event-stream".to_string());

    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(32);
    let db = state.db.clone();
    let pool_key_id = auth.id;
    let user_id = auth.user_id;
    tokio::spawn(async move {
        let mut upstream = response.bytes_stream();
        let mut meter = SseMeter::new(strip_injected_usage);
        while let Some(chunk) = upstream.next().await {
            let Ok(bytes) = chunk else {
                tracing::warn!("upstream stream aborted");
                break;
            };
            let forward = meter.feed(&bytes);
            if !forward.is_empty() && tx.send(Ok(Bytes::from(forward))).await.is_err() {
                // Client went away; keep whatever was metered so far.
                break;
            }
        }
        let tail = meter.finish();
        if !tail.is_empty() {
            let _ = tx.send(Ok(Bytes::from(tail))).await;
        }
        drop(tx);

        let outcome: UsageOutcome = meter.outcome();
        let event = UsageEvent {
            id: db::new_id(),
            ts: db::now_ms(),
            pool_key_id,
            user_id,
            model: outcome.model.clone().unwrap_or(requested_model),
            prompt_tokens: outcome.prompt_tokens,
            completion_tokens: outcome.completion_tokens,
            cost_micros: 0,
            latency_ms: started.elapsed().as_millis().min(i64::MAX as u128) as i64,
            status: "200".to_string(),
        };
        if let Err(err) = db::insert_usage_event(&db, &event).await {
            // The response is already streaming; metering loss is logged loudly.
            tracing::error!("usage insert failed after stream: {err}");
        }
    });

    let body_stream =
        futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|b| (b, rx)) });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(body_stream))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("response build failed: {e}")))
}

/// Reads usage/model out of a buffered (non-streaming or error) upstream body.
fn parse_nonstream_usage(bytes: &[u8]) -> UsageOutcome {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return UsageOutcome {
            prompt_tokens: 0,
            completion_tokens: 0,
            model: None,
            exact: false,
        };
    };
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    match value.get("usage").filter(|u| u.is_object()) {
        Some(usage) => UsageOutcome {
            prompt_tokens: usage
                .get("prompt_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            completion_tokens: usage
                .get("completion_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            model,
            exact: true,
        },
        None => UsageOutcome {
            prompt_tokens: 0,
            completion_tokens: 0,
            model,
            exact: false,
        },
    }
}
