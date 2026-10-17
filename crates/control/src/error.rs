//! Control-plane errors, rendered as `{"error": {message, code}}` JSON.
//!
//! Distinct from the gateway's OpenAI-shaped error: this API is admin-only
//! tooling, so codes are our own (`unauthorized`, `not_found`, `conflict`…)
//! and internal details never reach the client (logged server-side only).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Every failure mode the control plane surfaces to HTTP clients.
#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    /// Missing/invalid credentials — one generic message for all causes so
    /// login probing learns nothing (which field was wrong).
    #[error("unauthorized")]
    Unauthorized,
    /// Authenticated but not allowed to do this (e.g. member self-service, M3).
    #[error("forbidden")]
    Forbidden,
    /// Resource does not exist *within the caller's scope* — also returned
    /// for other orgs' resources so existence is not leaked.
    #[error("not found")]
    NotFound,
    /// Malformed input; the message is safe to show (it quotes the field).
    #[error("{0}")]
    BadRequest(String),
    /// State conflict (e.g. deleting an org that still has pools).
    #[error("{0}")]
    Conflict(String),
    /// Anything unexpected — details stay in server logs.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Maps each error to its HTTP status + JSON body.
impl IntoResponse for ControlError {
    fn into_response(self) -> Response {
        // Internal errors are logged BEFORE building the response; the body
        // never carries the detail (could contain SQL or key material paths).
        if let Self::Internal(err) = &self {
            tracing::error!("control-plane internal error: {err:#}");
        }
        let (status, code, message): (StatusCode, &str, String) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication required.".to_string(),
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "You may not perform this action.".to_string(),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Resource not found.".to_string(),
            ),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg),
            Self::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The server had an error while processing your request.".to_string(),
            ),
        };
        let body = json!({ "error": { "message": message, "code": code } });
        (status, Json(body)).into_response()
    }
}

/// Convenience alias for handler return types.
pub type ApiResult<T> = Result<T, ControlError>;
