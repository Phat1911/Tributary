//! OpenAI-compatible error responses.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Errors surfaced by the data plane, always rendered in the OpenAI error shape.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    BadGateway(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, typ, code, message): (StatusCode, &str, &str, String) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "invalid_request_error",
                "invalid_api_key",
                "Incorrect API key provided.".to_string(),
            ),
            Self::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "invalid_request_error",
                msg,
            ),
            Self::BadGateway(msg) => (StatusCode::BAD_GATEWAY, "api_error", "upstream_error", msg),
            Self::Internal(err) => {
                // Details stay server-side; clients get a generic 500.
                tracing::error!("internal error: {err:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "api_error",
                    "internal_error",
                    "The server had an error while processing your request.".to_string(),
                )
            }
        };

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
}
