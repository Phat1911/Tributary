//! Pool-key request authentication for the data plane.
//!
//! Key generation/hashing moved to `tributary_common::keys` in M2 (the
//! control plane issues the same keys); this module keeps the HTTP-facing
//! pieces: bearer parsing and the `AuthedKey` extractor, now pool-scoped.
//!
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
/// (SPEC §5 lifecycle step 2): valid, active, unexpired, AND bound to a
/// pool — poolless keys are M1 relics with nothing to route to.
#[derive(Debug, Clone)]
pub struct AuthedKey {
    pub id: String,
    pub user_id: Option<String>,
    /// The pool this key unlocks (never NULL post-auth).
    pub pool_id: String,
    /// Masked display form — detection payloads carry this, never raw
    /// material (AGENTS secrets convention).
    pub key_prefix: String,
}

impl FromRequestParts<AppState> for AuthedKey {
    /// The associated error type returned when extraction fails — here the
    /// data plane's OpenAI-shaped error (401 on any auth failure).
    type Rejection = ApiError;

    /// Runs before the handler: parse the bearer token, hash it, look it up,
    /// and enforce status/expiry/pool-binding. Any failure rejects.
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
        if row.status == "suspended" {
            // Auto-block recovery UX (D28): the member must be told WHY,
            // unlike an attacker-facing 401. Keys are 256-bit random —
            // enumerating them to distinguish 401 vs 403 is infeasible.
            return Err(ApiError::Forbidden(
                "This key is suspended pending review by your administrator. They can \
                 restore it from the review queue."
                    .to_string(),
            ));
        }
        if row.status != "active" {
            return Err(ApiError::Unauthorized);
        }
        if let Some(expires_at) = row.expires_at
            && db::now_ms() >= expires_at
        {
            return Err(ApiError::Unauthorized);
        }
        let pool_id = row.pool_id.filter(|p| !p.is_empty()).ok_or_else(|| {
            // Keys predating M2 have no pool; they cannot route anywhere.
            ApiError::Unauthorized
        })?;
        Ok(AuthedKey {
            id: row.id,
            user_id: row.user_id,
            pool_id,
            key_prefix: row.key_prefix,
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
