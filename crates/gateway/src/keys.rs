//! Pool-key request authentication for the data plane.
//!
//! Key generation/hashing moved to `tributary_common::keys` in M2 (the
//! control plane issues the same keys); this module keeps the HTTP-facing
//! pieces: bearer parsing and the `AuthedKey` extractor.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use tributary_common::keys as common_keys;

use crate::db::{self, PoolKeyRow};
use crate::error::ApiError;
use crate::state::AppState;

// Re-exported from `common` so existing `tributary_gateway::keys::*` users
// (binary subcommands, tests) are unchanged by the M2 move.
pub use tributary_common::keys::{
    KEY_PREFIX, constant_time_hash_eq, generate_key, hash_token, is_plausible_pool_key,
};

/// Extracts the bearer token from request headers, `None` if absent/malformed.
pub fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme == "Bearer").then_some(token)
}

/// An authenticated pool key, extracted and validated per request
/// (SPEC §5 lifecycle step 2 — M1 subset: valid, active, unexpired).
#[derive(Debug, Clone)]
pub struct AuthedKey {
    pub id: String,
    pub user_id: Option<String>,
}

impl FromRequestParts<AppState> for AuthedKey {
    /// The associated error type returned when extraction fails — here the
    /// data plane's OpenAI-shaped error (401 on any auth failure).
    type Rejection = ApiError;

    /// Runs before the handler: parse the bearer token, hash it, look it up,
    /// and enforce status/expiry. Any failure rejects the request.
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let raw = bearer_token(&parts.headers).ok_or(ApiError::Unauthorized)?;
        if !common_keys::is_plausible_pool_key(raw) {
            return Err(ApiError::Unauthorized);
        }
        let computed = hash_token(raw);
        let row: PoolKeyRow = db::lookup_pool_key(&state.db, &computed)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("pool key lookup failed: {e}")))?
            .ok_or(ApiError::Unauthorized)?;
        if !constant_time_hash_eq(&row.key_hash, &computed) {
            return Err(ApiError::Unauthorized);
        }
        if row.status != "active" {
            return Err(ApiError::Unauthorized);
        }
        if let Some(expires_at) = row.expires_at
            && db::now_ms() >= expires_at
        {
            return Err(ApiError::Unauthorized);
        }
        Ok(AuthedKey {
            id: row.id,
            user_id: row.user_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_parsing_rejects_bad_schemes() {
        let mut headers = axum::http::HeaderMap::new();
        assert!(bearer_token(&headers).is_none());
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Basic sk-pool-abc"),
        );
        assert!(bearer_token(&headers).is_none());
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer sk-pool-abc"),
        );
        assert_eq!(bearer_token(&headers), Some("sk-pool-abc"));
    }
}
