//! OpenAI-compatible error responses.

use axum::Json;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Errors surfaced by the data plane, always rendered in the OpenAI error shape.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("{0}")]
    BadRequest(String),
    /// 403 — authenticated but blocked (e.g. pool disabled, model not allowed).
    #[error("{0}")]
    Forbidden(String),
    /// 429 — quota or rate limit hit BEFORE any upstream call (rule 4).
    #[error("{message}")]
    TooManyRequests {
        message: String,
        /// Seconds until the limiting window resets (Retry-After header).
        retry_after_secs: i64,
    },
    /// 503 — no upstream capacity available (e.g. pool has no active keys).
    #[error("{0}")]
    ServiceUnavailable(String),
    #[error("{0}")]
    BadGateway(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Builds the OpenAI-shaped error envelope with status/type/code fields.
fn envelope(status: StatusCode, typ: &str, code: &str, message: String) -> Response {
    let body = json!({
        "error": {
            "message": message,
            "type": typ,
            "param": null,
            "code": code,
        }
    });
    (status, Json(body)).into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized => envelope(
                StatusCode::UNAUTHORIZED,
                "invalid_request_error",
                "invalid_api_key",
                "Incorrect API key provided.".to_string(),
            ),
            Self::BadRequest(msg) => envelope(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "invalid_request_error",
                msg,
            ),
            Self::Forbidden(msg) => envelope(
                StatusCode::FORBIDDEN,
                "invalid_request_error",
                "forbidden",
                msg,
            ),
            Self::TooManyRequests {
                message,
                retry_after_secs,
            } => {
                let mut response = envelope(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limit_error",
                    "rate_limit_exceeded",
                    message,
                );
                // Standard header clients already understand (OpenAI SDK does).
                if let Ok(value) = retry_after_secs.to_string().parse() {
                    response.headers_mut().insert(header::RETRY_AFTER, value);
                }
                response
            }
            Self::ServiceUnavailable(msg) => envelope(
                StatusCode::SERVICE_UNAVAILABLE,
                "api_error",
                "no_upstream",
                msg,
            ),
            Self::BadGateway(msg) => {
                envelope(StatusCode::BAD_GATEWAY, "api_error", "upstream_error", msg)
            }
            Self::Internal(err) => {
                // Details stay server-side; clients get a generic 500.
                tracing::error!("internal error: {err:#}");
                envelope(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "api_error",
                    "internal_error",
                    "The server had an error while processing your request.".to_string(),
                )
            }
        }
    }
}
