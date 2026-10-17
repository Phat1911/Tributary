//! Admin authentication: argon2 password hashing + hashed session tokens.
//!
//! D8 (built-in lightweight auth): passwords are stored as argon2id PHC
//! strings; login mints an opaque `ses-…` token whose SHA-256 hash is the
//! only thing persisted — the same store-never-the-secret hygiene as pool
//! keys. M2 uses bearer tokens (no cookie surface until the M3 dashboard).

use argon2::Argon2;
use argon2::PasswordHash;
use argon2::PasswordHasher;
use argon2::PasswordVerifier;
use argon2::password_hash::SaltString;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rand::RngCore;
use tributary_common::keys::hash_token;

use crate::ControlState;
use crate::SESSION_TTL_MS;
use crate::db;
use crate::error::ControlError;

/// Hashes a password with argon2id (default params: m=19 MiB, t=2, p=1) and
/// a fresh random salt, returning the portable PHC string for storage.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    // The salt must be unique per password; we generate it ourselves (16
    // bytes of OS randomness base64-encoded into the PHC string).
    let mut salt_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| anyhow::anyhow!("salt encoding failed: {e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?
        .to_string())
}

/// Verifies a password against a stored PHC string. Any parse/hash failure
/// returns `false` — callers see one uniform "bad credentials".
pub fn verify_password(stored_phc: &str, presented: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_phc) else {
        return false;
    };
    Argon2::default()
        .verify_password(presented.as_bytes(), &parsed)
        .is_ok()
}

/// Enforces basic credential hygiene before any hashing work.
pub fn validate_credentials(email: &str, password: &str) -> Result<(), ControlError> {
    let email = email.trim();
    if !email.contains('@') || email.len() < 3 || email.len() > 320 {
        return Err(ControlError::BadRequest(
            "email must be a valid address".into(),
        ));
    }
    if password.len() < 8 {
        return Err(ControlError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

/// Generates a fresh session token: returns `(raw, hash, expires_at_ms)`.
///
/// The raw token (`ses-<64 hex>`, 256 bits) leaves the process exactly once
/// in the login response; only the hash is stored.
pub fn generate_session_token() -> (String, String, i64) {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let raw = format!("ses-{}", hex::encode(bytes));
    let hash = hash_token(&raw);
    let expires_at = tributary_common::db::now_ms() + SESSION_TTL_MS;
    (raw, hash, expires_at)
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

/// The authenticated admin identity attached to a request.
#[derive(Debug, Clone)]
pub struct AuthedAdmin {
    pub user_id: String,
    /// `host_admin` sees everything; `org_admin` only their org's scope.
    pub role: String,
    pub org_id: Option<String>,
}

impl AuthedAdmin {
    /// Scope check: is this admin allowed to touch `org_id`'s resources?
    ///
    /// Returns `NotFound` (not `Forbidden`) for cross-org access so the
    /// response cannot be used to probe which org ids exist.
    pub fn ensure_org_scope(&self, org_id: &str) -> Result<(), ControlError> {
        if self.role == "host_admin" || self.org_id.as_deref() == Some(org_id) {
            Ok(())
        } else {
            Err(ControlError::NotFound)
        }
    }
}

/// Resolves the session bearer token into an admin user, rejecting anything
/// that is not a currently-valid admin session.
async fn resolve_session(state: &ControlState, token: &str) -> Result<AuthedAdmin, ControlError> {
    let hash = hash_token(token);
    let now = tributary_common::db::now_ms();
    let row = db::session_user_by_token_hash(&state.db, &hash)
        .await
        .map_err(|e| ControlError::Internal(anyhow::anyhow!("session lookup failed: {e}")))?
        .ok_or(ControlError::Unauthorized)?;
    if row.expires_at <= now {
        // Lazy expiry sweep: delete the dead row so tokens self-clean.
        let _ = db::delete_session(&state.db, &row.session_id).await;
        return Err(ControlError::Unauthorized);
    }
    // Members have no control-plane access until self-service (M3).
    if row.role != "host_admin" && row.role != "org_admin" {
        return Err(ControlError::Forbidden);
    }
    Ok(AuthedAdmin {
        user_id: row.user_id,
        role: row.role,
        org_id: row.org_id,
    })
}

/// Axum extractor: any handler taking `AuthedAdmin` is login-gated.
impl FromRequestParts<ControlState> for AuthedAdmin {
    /// The error produced when extraction (auth) fails.
    type Rejection = ControlError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ControlState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers).ok_or(ControlError::Unauthorized)?;
        resolve_session(state, token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hashing_roundtrips_and_rejects_wrong() {
        let phc = hash_password("correct horse battery staple").expect("hash");
        assert!(
            phc.starts_with("$argon2id$"),
            "PHC string encodes algorithm+params"
        );
        assert!(verify_password(&phc, "correct horse battery staple"));
        assert!(!verify_password(&phc, "wrong password"));
        assert!(!verify_password("not-a-phc-string", "anything"));
    }

    #[test]
    fn session_tokens_are_prefixed_and_hashed() {
        let (raw, hash, expires_at) = generate_session_token();
        assert!(raw.starts_with("ses-"));
        assert_ne!(raw, hash, "store the hash, never the raw token");
        assert!(hash.len() == 64, "SHA-256 hex digest");
        assert!(expires_at > tributary_common::db::now_ms());
    }

    #[test]
    fn credential_validation_bounds() {
        assert!(validate_credentials("a@b.co", "12345678").is_ok());
        assert!(validate_credentials("no-at-sign", "12345678").is_err());
        assert!(validate_credentials("a@b.co", "short").is_err());
    }

    #[test]
    fn org_scope_hides_rather_than_reveals() {
        let host = AuthedAdmin {
            user_id: "u1".into(),
            role: "host_admin".into(),
            org_id: None,
        };
        let org_a = AuthedAdmin {
            user_id: "u2".into(),
            role: "org_admin".into(),
            org_id: Some("org-a".into()),
        };
        assert!(host.ensure_org_scope("any-org").is_ok());
        assert!(org_a.ensure_org_scope("org-a").is_ok());
        assert!(matches!(
            org_a.ensure_org_scope("org-b"),
            Err(ControlError::NotFound)
        ));
    }
}
