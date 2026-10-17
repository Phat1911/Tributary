//! Control plane: REST admin API, auth, sessions (SPEC §7).
//!
//! M2 surface: login/logout (argon2 + hashed session tokens, D8 built-in
//! lightweight auth), orgs, members, pools, pool keys (issuance/revocation),
//! provider keys (envelope-encrypted at rest, validate-on-save health probe,
//! manual revalidate), and quota rules. No member self-service and no UI —
//! both arrive with M3. The router merges into the single gateway binary
//! (SPEC §13 "modular monolith").

pub mod api;
pub mod auth;
pub mod db;
pub mod error;

use axum::Router;
use axum::routing::{get, post};
use tributary_common::db::Db;
use tributary_crypto::MasterKey;

/// Shared control-plane state, cloned into every handler.
#[derive(Clone)]
pub struct ControlState {
    pub db: Db,
    /// HTTP client for provider-key validation probes (`GET {base_url}/models`).
    pub http: reqwest::Client,
    /// Master key used to seal provider keys at rest (rule 2).
    pub master: MasterKey,
    /// Default model allowlist for new pools when the request omits one
    /// (from `TRIBUTARY_MODELS`; identities live in config, never code — SPEC §15).
    pub default_models: Vec<String>,
}

/// Session lifetime. 24 h keeps daily admin flows token-free while bounding
/// the blast radius of a leaked session token (M4 tightens with rotation).
pub const SESSION_TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// Builds the `/api/v1` router with state applied — ready to merge into the
/// gateway's app (returns `Router<()>`, i.e. a router with no remaining state).
pub fn router(state: ControlState) -> Router {
    Router::new()
        .route("/api/v1/auth/login", post(api::login))
        .route("/api/v1/auth/logout", post(api::logout))
        .route("/api/v1/orgs", get(api::list_orgs).post(api::create_org))
        .route(
            "/api/v1/orgs/{id}",
            get(api::get_org).delete(api::delete_org),
        )
        .route(
            "/api/v1/members",
            get(api::list_members).post(api::create_member),
        )
        .route(
            "/api/v1/members/{id}",
            axum::routing::delete(api::delete_member),
        )
        .route("/api/v1/pools", get(api::list_pools).post(api::create_pool))
        .route(
            "/api/v1/pools/{id}",
            get(api::get_pool).delete(api::disable_pool),
        )
        .route(
            "/api/v1/pools/{id}/keys",
            get(api::list_pool_keys).post(api::issue_pool_key),
        )
        .route(
            "/api/v1/pools/{id}/keys/{key_id}",
            axum::routing::delete(api::revoke_pool_key),
        )
        .route(
            "/api/v1/pools/{id}/provider-keys",
            get(api::list_provider_keys).post(api::create_provider_key),
        )
        .route(
            "/api/v1/pools/{id}/provider-keys/{key_id}",
            axum::routing::delete(api::revoke_provider_key),
        )
        .route(
            "/api/v1/pools/{id}/provider-keys/{key_id}/revalidate",
            post(api::revalidate_provider_key),
        )
        .route(
            "/api/v1/quota-rules",
            get(api::list_quota_rules).post(api::upsert_quota_rule),
        )
        .route(
            "/api/v1/quota-rules/{id}",
            axum::routing::delete(api::delete_quota_rule),
        )
        .with_state(state)
}

/// Creates the first host_admin user; errors if any user exists.
///
/// Bootstrap-only by design (the CLI cannot silently mint more admins) —
/// further admins and members are created through the authenticated API.
pub async fn create_host_admin(
    db: &Db,
    email: &str,
    password: &str,
) -> anyhow::Result<db::UserRow> {
    if db::count_users(db).await? > 0 {
        anyhow::bail!("refusing to create host admin: users already exist — use the admin API");
    }
    auth::validate_credentials(email, password)?;
    let hash = auth::hash_password(password)?;
    let user = db::UserRow {
        id: tributary_common::db::new_id(),
        email: email.trim().to_lowercase(),
        password_hash: hash,
        role: "host_admin".to_string(),
        org_id: None,
        created_at: tributary_common::db::now_ms(),
    };
    db::insert_user(db, &user).await?;
    Ok(user)
}
