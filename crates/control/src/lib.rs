//! Control plane: REST admin API, auth, sessions (SPEC §7).
//!
//! M3 surface: everything M2 had (login/logout with argon2 + hashed session
//! tokens, orgs, members, pools, pool keys, provider keys, quota rules)
//! plus cookie sessions for the browser dashboard (secure HttpOnly,
//! SameSite=Lax — JSON-only mutations make the CSRF posture structural),
//! login throttling (AUDIT A-004 closed), the audit trail
//! (`audit_log` + `/api/v1/audit`), usage endpoints for dashboards, and
//! member self-service under `/api/v1/me/*`. The router merges into the
//! single gateway binary (SPEC §13 "modular monolith").
//!
//! M4 adds the leak-prevention surfaces (SPEC §9): key governance
//! (rotation, expiry, IP allowlists), the admin review
//! queue, email alerts (outbox + async SMTP/log-fallback), the detection
//! pipeline shared with the gateway, and the BYOK classifier config (D28).

pub mod api;
pub mod approvals;
pub mod audit;
pub mod auth;
pub mod classifier;
pub mod db;
pub mod error;
pub mod me;
pub mod notify;
pub mod security;
pub mod throttle;
pub mod usage;

use axum::Router;
use axum::routing::{get, post};
use tributary_common::db::Db;
use tributary_crypto::MasterKey;

/// Shared control-plane state, cloned into every handler.
#[derive(Clone)]
pub struct ControlState {
    pub db: Db,
    /// HTTP client for provider-key validation probes (`GET {base_url}/models`)
    /// and classifier save-time probes (D28).
    pub http: reqwest::Client,
    /// Master key used to seal provider keys at rest (rule 2).
    pub master: MasterKey,
    /// Default model allowlist for new pools when the request omits one
    /// (from `TRIBUTARY_MODELS`; identities live in config, never code — SPEC §15).
    pub default_models: Vec<String>,
    /// Login-throttle state (in-process fast path; A-004).
    pub login_throttle: throttle::LoginThrottle,
    /// Failed logins allowed per (email, ip) per fixed 15-minute window.
    /// A field (not a constant) so tests can tighten it per-app; default 5.
    pub login_max_fails: i64,
    /// SMTP relay for detection emails; `None` ⇒ log-fallback (M4).
    pub smtp: Option<notify::SmtpConfig>,
}

impl ControlState {
    /// Builds state with production defaults (throttle: 5 fails / window;
    /// SMTP read from `TRIBUTARY_SMTP_*` — absent ⇒ fallback path).
    pub fn new(
        db: Db,
        http: reqwest::Client,
        master: MasterKey,
        default_models: Vec<String>,
    ) -> Self {
        Self {
            db,
            http,
            master,
            default_models,
            login_throttle: throttle::LoginThrottle::new(),
            login_max_fails: 5,
            smtp: notify::SmtpConfig::from_env(),
        }
    }
}

/// Session lifetime. 24 h keeps daily admin flows token-free while bounding
/// the blast radius of a leaked session token (M4 tightens with rotation).
pub const SESSION_TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// Builds the `/api/v1` router with state applied — ready to merge into the
/// gateway's app (returns `Router<()>`, i.e. a router with no remaining state).
/// Builds the control-plane router. The scrub runs once per boot
/// (fire-and-forget): legacy `approval.requested` audit rows written
/// before b012cb5 still carry sealed-blob fields — A-010's fix covers
/// new writes; this clears the old ones idempotently.
pub fn router(state: ControlState) -> Router {
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let _ = crate::audit::scrub_legacy_approval_audit(&db).await;
        });
    }
    Router::new()
        .route("/api/v1/auth/login", post(api::login))
        .route("/api/v1/auth/logout", post(api::logout))
        .route("/api/v1/auth/me", get(api::whoami))
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
        .route("/api/v1/pools/{id}/keys/manage", get(api::manage_pool_keys))
        .route(
            "/api/v1/pools/{id}/keys/{key_id}",
            axum::routing::delete(api::revoke_pool_key),
        )
        .route(
            "/api/v1/pools/{id}/keys/{key_id}/rotate",
            post(api::rotate_pool_key),
        )
        .route(
            "/api/v1/pools/{id}/keys/{key_id}/expiry",
            post(api::set_key_expiry),
        )
        .route(
            "/api/v1/pools/{id}/keys/{key_id}/allowlists",
            get(api::list_key_allowlists).post(api::add_key_allowlist),
        )
        .route(
            "/api/v1/pools/{id}/keys/{key_id}/allowlists/{entry_id}",
            axum::routing::delete(api::remove_key_allowlist),
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
        .route("/api/v1/usage", get(usage::list_for_pool))
        .route("/api/v1/usage/summary", get(usage::summary))
        .route("/api/v1/me/keys", get(me::my_keys))
        .route(
            "/api/v1/me/keys/{key_id}",
            axum::routing::delete(me::revoke_my_key),
        )
        .route("/api/v1/me/usage", get(me::my_usage))
        .route("/api/v1/me/usage/summary", get(me::my_usage_summary))
        .route("/api/v1/me/disclosure", get(me::my_disclosure))
        // D33 admin four-eyes: the pending-action list + decision API.
        .route(
            "/api/v1/orgs/{id}/pending-actions",
            get(api::list_pending_actions),
        )
        .route(
            "/api/v1/orgs/{id}/pending-actions/{action_id}/approve",
            post(api::approve_pending_action),
        )
        .route(
            "/api/v1/orgs/{id}/pending-actions/{action_id}/reject",
            post(api::reject_pending_action),
        )
        .route(
            "/api/v1/orgs/{id}/admins/{user_id}/demote",
            post(api::demote_admin),
        )
        .route("/api/v1/audit", get(api::list_audit))
        .route("/api/v1/review-flags", get(api::list_review_flags))
        .route(
            "/api/v1/review-flags/{id}/resolve",
            post(api::resolve_review_flag),
        )
        .route(
            "/api/v1/orgs/{id}/settings",
            get(api::get_org_settings).put(api::put_org_settings),
        )
        .route(
            "/api/v1/orgs/{id}/classifier",
            get(api::get_classifier)
                .put(api::put_classifier)
                .delete(api::delete_classifier),
        )
        .route(
            "/api/v1/orgs/{id}/classifier/go-live",
            post(api::classifier_go_live),
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
