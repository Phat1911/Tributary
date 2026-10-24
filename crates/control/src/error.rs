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
    /// A-022b (M5): the org has ≥2 admins, so this D33-class action may
    /// not execute via the direct endpoint — it must be REQUESTED and
    /// approved through the pending-actions queue. The body names the
    /// kind and the request path so a client can act without guessing.
    #[error("approval required")]
    ApprovalRequired {
        /// The pending-action `kind` to POST (e.g. `admin_key_revoke`).
        kind: String,
        /// The org-scoped request endpoint, verbatim.
        request_uri: String,
    },
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
    /// Rate-limited (login throttle, A-004) — carries the `Retry-After`.
    #[error("too many requests")]
    TooManyRequests {
        /// Whole seconds until the fixed window resets.
        retry_after_secs: i64,
    },
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
        // The throttle denial additionally sends a standard Retry-After.
        let retry_after = match &self {
            Self::TooManyRequests { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        };
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
            Self::ApprovalRequired { kind, request_uri } => (
                StatusCode::FORBIDDEN,
                "approval_required",
                // The actionable hint IS the contract (A-022b): direct
                // endpoints have no write effect in dual-admin orgs; the
                // write path is the approvals queue.
                format!(
                    "This org requires a second admin's approval for '{kind}'. \
                     POST {{\"kind\": \"{kind}\", \"params\": {{…}}}} to {request_uri}."
                ),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Resource not found.".to_string(),
            ),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg),
            Self::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg),
            Self::TooManyRequests { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many attempts. Try again later.".to_string(),
            ),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The server had an error while processing your request.".to_string(),
            ),
        };
        let body = json!({ "error": { "message": message, "code": code } });
        let mut response = (status, Json(body)).into_response();
        if let Some(secs) = retry_after {
            // Standard header the dashboard surfaces as a countdown hint.
            if let Ok(value) = axum::http::HeaderValue::from_str(&secs.to_string()) {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, value);
            }
        }
        response
    }
}

/// Convenience alias for handler return types.
pub type ApiResult<T> = Result<T, ControlError>;
