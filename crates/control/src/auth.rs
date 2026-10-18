//! Admin authentication: argon2 password hashing + hashed session tokens.
//!
//! D8 (built-in lightweight auth): passwords are stored as argon2id PHC
//! strings; login mints an opaque `ses-…` token whose SHA-256 hash is the
//! only thing persisted — the same store-never-the-secret hygiene as pool
//! keys. Since M3 the SAME session token is presented two ways: as a bearer
//! header (API/CLI) or as a secure HttpOnly cookie (dashboard browser).

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

/// Session cookie name (browser auth, M3). Path is pinned to the API prefix
/// so the browser never ships it to page routes — smallest surface that works.
pub const SESSION_COOKIE: &str = "tributary_session";

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

/// Extracts the session token from the `Cookie` header, `None` if absent.
///
/// Cookie format after parsing: one `k=v` pair per `; ` separator — the
/// browser standard. We scan rather than split so malformed pairs from other
/// cookies can't break the lookup.
pub fn cookie_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == SESSION_COOKIE).then_some(value)
    })
}

/// Builds the `Set-Cookie` header value for a fresh browser session.
///
/// `HttpOnly` + `Secure` + `SameSite=Lax` per SPEC §7: the cookie is
/// invisible to page JS, only travels over TLS (localhost is exempt by
/// modern browsers — see the PLAN dev note), and — Lax — is never attached
/// to cross-site POSTs, which is the CSRF posture for all cookie-auth'd
/// mutations (they are additionally JSON-only; forms can't target them).
pub fn session_cookie(raw: &str, max_age_secs: i64) -> String {
    format!(
        "{SESSION_COOKIE}={raw}; Path=/api/v1; Max-Age={max_age_secs}; \
         HttpOnly; Secure; SameSite=Lax"
    )
}

/// Builds the `Set-Cookie` header value that clears the session cookie
/// (logout): same name/path, already expired, empty value.
pub fn expired_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/api/v1; Max-Age=0; HttpOnly; Secure; SameSite=Lax")
}

/// Best-effort client IP for audit rows and throttle buckets: the FIRST
/// `X-Forwarded-For` hop (set by the deployment's reverse proxy, SPEC §8
/// TLS edge) or `"local"` for direct un-proxied connections.
///
/// Trust model: only the edge proxy writes XFF in a sane deployment; a
/// client-forged XFF can shift its own bucket to a fake IP, but not evade
/// the per-email dimension of the (email, ip) bucket. M4: the logic moved
/// to `tributary_common::net` so the gateway's IP allowlist shares it;
/// kept here as a re-export-shaped wrapper for existing callers.
pub fn client_ip(headers: &axum::http::HeaderMap) -> String {
    tributary_common::net::client_ip(headers)
}

/// Any authenticated user (admin or member) — the base identity extractors
/// build on. Members authenticate for self-service only (`/api/v1/me/*`).
#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub user_id: String,
    pub email: String,
    /// `host_admin` sees everything; `org_admin` their org; `member` themselves.
    pub role: String,
    pub org_id: Option<String>,
}

impl AuthedUser {
    /// Both admin roles may use the management endpoints.
    pub fn is_admin(&self) -> bool {
        self.role == "host_admin" || self.role == "org_admin"
    }
}

/// Resolves a session token (raw form) into a user, rejecting anything that
/// is not a currently-valid session.
async fn resolve_session(state: &ControlState, token: &str) -> Result<AuthedUser, ControlError> {
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
    Ok(AuthedUser {
        user_id: row.user_id,
        email: row.email,
        role: row.role,
        org_id: row.org_id,
    })
}

/// Extracts the session token from bearer header OR cookie (either surface).
pub fn any_token(headers: &axum::http::HeaderMap) -> Option<String> {
    bearer_token(headers)
        .map(str::to_string)
        .or_else(|| cookie_token(headers).map(str::to_string))
}

/// Axum extractor: any handler taking `AuthedUser` is login-gated and
/// member-compatible (self-service endpoints).
impl FromRequestParts<ControlState> for AuthedUser {
    /// The error produced when extraction (auth) fails.
    type Rejection = ControlError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ControlState,
    ) -> Result<Self, Self::Rejection> {
        let token = any_token(&parts.headers).ok_or(ControlError::Unauthorized)?;
        resolve_session(state, &token).await
    }
}

/// The authenticated admin identity attached to a request.
#[derive(Debug, Clone)]
pub struct AuthedAdmin {
    pub user_id: String,
    /// Snapshot for audit rows (the trail outlives the user row).
    pub email: String,
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

/// Axum extractor: handlers taking `AuthedAdmin` are admin-only (members
/// get a uniform `Forbidden` before any handler logic runs).
impl FromRequestParts<ControlState> for AuthedAdmin {
    /// The error produced when extraction (auth/role) fails.
    type Rejection = ControlError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ControlState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthedUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(ControlError::Forbidden);
        }
        Ok(AuthedAdmin {
            user_id: user.user_id,
            email: user.email,
            role: user.role,
            org_id: user.org_id,
        })
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
            email: "host@x.co".into(),
            role: "host_admin".into(),
            org_id: None,
        };
        let org_a = AuthedAdmin {
            user_id: "u2".into(),
            email: "a@x.co".into(),
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

    #[test]
    fn cookie_header_parsing_finds_our_session_among_neighbors() {
        use axum::http::HeaderMap;
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "theme=dark; tributary_session=ses-abc; other=1"
                .parse()
                .unwrap(),
        );
        assert_eq!(cookie_token(&headers), Some("ses-abc"));
        // Absent cookie → None (bearer-only client).
        assert_eq!(cookie_token(&HeaderMap::new()), None);
    }

    #[test]
    fn session_cookie_carries_security_attributes() {
        let cookie = session_cookie("ses-xyz", 3600);
        assert!(cookie.starts_with("tributary_session=ses-xyz;"));
        for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/api/v1"] {
            assert!(cookie.contains(attr), "missing {attr} in {cookie}");
        }
        assert!(expired_session_cookie().contains("Max-Age=0"));
    }

    #[test]
    fn client_ip_prefers_first_xff_hop() {
        use axum::http::HeaderMap;
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "9.9.9.9, 10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&headers), "9.9.9.9");
        assert_eq!(client_ip(&HeaderMap::new()), "local");
    }
}
