//! Pool-key material: generation, hashing, and request authentication.
//!
//! Raw keys look like `sk-pool-<64 hex>` (256 bits of entropy). Only the
//! SHA-256 hex digest is ever stored or queried; lookups go through the unique
//! `key_hash` index and the stored digest is additionally verified with a
//! constant-time comparison (defense in depth).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::db::{self, PoolKeyRow};
use crate::error::ApiError;
use crate::state::AppState;

pub const KEY_PREFIX: &str = "sk-pool-";
/// Minimum length of the random part (hex chars) we are willing to consider.
const MIN_BODY_LEN: usize = 16;

/// A freshly generated pool key. `raw` exists exactly once, at creation time.
pub struct GeneratedKey {
    pub raw: String,
    pub key_hash: String,
    /// Display form: `sk-pool-<first5>…<last4>` (secrets convention).
    pub key_prefix: String,
}

pub fn generate_key() -> GeneratedKey {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let body = hex::encode(bytes);
    let raw = format!("{KEY_PREFIX}{body}");
    let key_hash = hash_token(&raw);
    let key_prefix = format!("{KEY_PREFIX}{}…{}", &body[..5], &body[body.len() - 4..]);
    GeneratedKey {
        raw,
        key_hash,
        key_prefix,
    }
}

/// SHA-256 of the raw token, hex-encoded (what gets stored/queried).
pub fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Constant-time equality of two hex digests (subtle).
pub fn constant_time_hash_eq(stored_hex: &str, computed_hex: &str) -> bool {
    match (hex::decode(stored_hex), hex::decode(computed_hex)) {
        (Ok(a), Ok(b)) => bool::from(a.ct_eq(&b)),
        _ => false,
    }
}

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
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let raw = bearer_token(&parts.headers).ok_or(ApiError::Unauthorized)?;
        let body_len = raw.len().saturating_sub(KEY_PREFIX.len());
        if !raw.starts_with(KEY_PREFIX) || body_len < MIN_BODY_LEN {
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
    fn generated_keys_have_expected_shape() {
        let key = generate_key();
        assert!(key.raw.starts_with("sk-pool-"));
        assert_eq!(key.raw.len(), KEY_PREFIX.len() + 64);
        assert_eq!(key.key_hash.len(), 64);
        // Display form reveals only 9 characters of key material.
        assert!(key.key_prefix.starts_with("sk-pool-") && key.key_prefix.contains('…'));
        let visible: usize = key
            .key_prefix
            .trim_start_matches(KEY_PREFIX)
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .count();
        assert_eq!(visible, 9);
    }

    #[test]
    fn hashing_is_deterministic_and_salt_free_but_keyed_by_full_token() {
        let a = hash_token("sk-pool-abc");
        let b = hash_token("sk-pool-abc");
        let c = hash_token("sk-pool-abd");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Hash never contains the raw token.
        assert!(!a.contains("sk-pool"));
    }

    #[test]
    fn constant_time_compare_matches_equality() {
        let a = hash_token("x");
        let b = hash_token("x");
        let c = hash_token("y");
        assert!(constant_time_hash_eq(&a, &b));
        assert!(!constant_time_hash_eq(&a, &c));
        assert!(!constant_time_hash_eq(&a, "not-hex"));
    }

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
