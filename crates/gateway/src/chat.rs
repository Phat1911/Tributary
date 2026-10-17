//! `POST /v1/chat/completions` — OpenAI-compatible proxy with metering.
//!
//! Full SPEC §5 lifecycle as of M2: (1) auth (pool-scoped key); (2) pool
//! house rules (status + model allowlist); (3) quota/rate check BEFORE any
//! upstream work (rule 4); (4) provider-key selection (first-available) +
//! per-use decrypt; (5) forward/stream with token extraction; (6) append
//! `usage_events` (now attributing pool + provider key); (7) health signals
//! (M4+).

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
use crate::quota::{Decision, QuotaSubject};
use crate::routing;
use crate::sse::{SseMeter, UsageOutcome};
use crate::state::AppState;

/// Non-streaming calls may buffer; streaming calls must never hit it.
const NON_STREAM_TIMEOUT: Duration = Duration::from_secs(120);

/// Everything resolved during admission (before any upstream call).
struct Admission {
    requested_model: String,
    is_stream: bool,
    strip_injected_usage: bool,
    provider_key_id: String,
}

pub async fn chat_completions(
    State(state): State<AppState>,
    auth: AuthedKey,
    body: Bytes,
) -> Result<Response, ApiError> {
    let started = Instant::now();
    let mut request_body: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::BadRequest(format!("invalid JSON body: {e}")))?;

    // --- House rules: pool must be active, model must be allowed. --------
    let pool = routing::pool_info(&state.db, &auth.pool_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("pool lookup failed: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized)?;
    if pool.status != "active" {
        return Err(ApiError::Forbidden("This pool is disabled.".to_string()));
    }
    let requested_model = request_body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    if !pool.allowlist.is_empty() && !pool.allowlist.iter().any(|m| m == &requested_model) {
        // House rule (SPEC §3.1 "model allowlists") — rejected locally,
        // upstream never sees the request.
        return Err(ApiError::BadRequest(format!(
            "Model '{requested_model}' is not allowed for this pool."
        )));
    }

    // --- Rule 4: quota + rate check BEFORE any upstream call. ------------
    let subject = QuotaSubject {
        pool_key_id: auth.id.clone(),
        user_id: auth.user_id.clone(),
        pool_id: auth.pool_id.clone(),
    };
    let now = db::now_ms();
    if let Decision::Deny {
        message,
        reset_at_ms,
    } = state
        .quota
        .check(&state.db, &subject, now)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("quota check failed: {e}")))?
    {
        let retry_after_secs = ((reset_at_ms - now) / 1000).max(1);
        return Err(ApiError::TooManyRequests {
            message,
            retry_after_secs,
        });
    }

    // --- Key selection + per-use decrypt (SPEC §5 steps 4–5). ------------
    let selected = routing::select_provider_key(&state.db, &auth.pool_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("provider key selection failed: {e}")))?
        .ok_or_else(|| {
            ApiError::ServiceUnavailable("No provider key is available for this pool.".to_string())
        })?;
    let upstream_key = routing::decrypt_provider_key(&state.config.master_key, &selected)?;

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

    // Admitted: count the request NOW (in-proc counters), then forward.
    state.quota.record_request(&subject, now);

    let url = format!("{}/chat/completions", selected.base_url);
    let mut upstream = state
        .http
        .post(&url)
        .bearer_auth(upstream_key.as_str())
        .json(&request_body);
    if !is_stream {
        upstream = upstream.timeout(NON_STREAM_TIMEOUT);
    }
    let response = upstream.send().await.map_err(|e| {
        tracing::warn!("upstream request failed: {e:#}");
        ApiError::BadGateway("upstream request failed".to_string())
    })?;

    let admission = Admission {
        requested_model,
        is_stream,
        strip_injected_usage,
        provider_key_id: selected.id,
    };

    let status = response.status();
    if !admission.is_stream || !status.is_success() {
        // Buffered path: non-streaming responses and upstream errors.
        buffered_response(state, auth, admission, response, started).await
    } else {
        // Streaming success: SSE passthrough with metering.
        streaming_response(state, auth, admission, response, started).await
    }
}

/// Forwards a fully-buffered upstream response (non-streaming or any error)
/// and records its usage event.
async fn buffered_response(
    state: AppState,
    auth: AuthedKey,
    admission: Admission,
    response: reqwest::Response,
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
    let model = outcome.model.clone().unwrap_or(admission.requested_model);
    let event = UsageEvent {
        id: db::new_id(),
        ts: db::now_ms(),
        pool_key_id: auth.id,
        user_id: auth.user_id,
        pool_id: Some(auth.pool_id),
        provider_key_id: Some(admission.provider_key_id),
        model,
        prompt_tokens: outcome.prompt_tokens,
        completion_tokens: outcome.completion_tokens,
        cost_micros: 0, // pricing table lands with D23
        latency_ms: started.elapsed().as_millis().min(i64::MAX as u128) as i64,
        status: status.as_u16().to_string(),
    };
    if let Err(err) = db::insert_usage_event(&state.db, &event).await {
        // Metering is the product — fail loudly rather than drop rows.
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
async fn streaming_response(
    state: AppState,
    auth: AuthedKey,
    admission: Admission,
    response: reqwest::Response,
    started: Instant,
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
    let pool_id = auth.pool_id;
    let provider_key_id = admission.provider_key_id;
    let requested_model = admission.requested_model;
    let strip = admission.strip_injected_usage;
    tokio::spawn(async move {
        let mut upstream = response.bytes_stream();
        let mut meter = SseMeter::new(strip);
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
            pool_id: Some(pool_id),
            provider_key_id: Some(provider_key_id),
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
