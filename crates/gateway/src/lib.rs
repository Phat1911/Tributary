//! Data plane: OpenAI-compatible proxy with pool-key auth and usage metering.
//!
//! M2 surface: `POST /v1/chat/completions` (streaming SSE passthrough and
//! non-streaming) and `GET /v1/models` (pool allowlist), per SPEC §7, plus
//! the merged control plane at `/api/v1` (SPEC §13 modular monolith — one
//! binary). Storage is sqlx `Any` — one portable query path over SQLite and
//! PostgreSQL.
//!
//! M4 adds the leak-prevention data-plane gates (SPEC §9): per-key IP
//! allowlists (fail-closed), per-key IP history with the dormant
//! impossible-travel check, and async sampled purpose classification
//! (`classifier`, D28).

pub mod chat;
pub mod classifier;
pub mod config;
pub mod db;
pub mod error;
pub mod geo;
pub mod keys;
pub mod models_api;
pub mod quota;
pub mod routing;
pub mod sse;
pub mod state;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use config::Config;
use state::AppState;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tributary_common::db::Db;
use tributary_control::ControlState;

/// Builds the data-plane router over shared state.
pub fn build_app(state: AppState) -> Router {
    // A-019 client-IP sanitization: capture the trusted list before
    // with_state moves the AppState; raw XFF dies at this layer.
    let trusted = state.config.trusted_proxies.clone();
    Router::new()
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/models", get(models_api::list_models))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            Arc::new(trusted),
            tributary_control::client_ip::sanitize_xff,
        ))
}

/// Connects the DB, runs migrations, and builds a reqwest client.
pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let db_pool = db::Db::connect(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?;
    Ok(AppState::new(db_pool, http, config))
}

/// Assembles the FULL single-binary app: data plane + control plane over one
/// DB pool (SPEC §13). `serve` and the integration tests both use this.
pub async fn build_full_app(db: Db, config: Config) -> anyhow::Result<Router> {
    db::run_migrations(&db).await?;
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?;
    let app_state = AppState::new(db.clone(), http.clone(), config.clone());
    let mut control_state = ControlState::new(
        db,
        http,
        config.master_key.clone(),
        config.default_models.clone(),
    );
    // A-019: both planes share ONE trust decision — the control router
    // sanitizes XFF with the same trusted-proxy list as the data plane.
    control_state.trusted_proxies = config.trusted_proxies.clone();
    // /healthz (M5): the compose healthcheck's target. Unauthenticated by
    // design — it leaks no data (a SELECT 1 + status) and must answer
    // before any admin exists; DB down ⇒ 503 so orchestration restarts
    // the container instead of routing to a zombie. State is CAPTURED
    // (the merged router is stateless — an extractor would find no
    // provider), so the probe owns its own Db clone.
    let healthz_db = app_state.db.clone();
    // Dashboard static export (M5 packaging): when TRIBUTARY_STATIC_DIR
    // points at a Next.js `out/` directory, it is served at "/" — the
    // container sets the default (/usr/share/tributary/dashboard);
    // dev/blank deployments leave it unset and skip the layer entirely.
    let static_dir = config.static_dir.clone();
    let mut app = build_app(app_state)
        .merge(tributary_control::router(control_state))
        .route(
            "/healthz",
            get(move || {
                let db = healthz_db.clone();
                async move {
                    match db::ping(&db).await {
                        Ok(true) => axum::http::StatusCode::OK,
                        _ => axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    }
                }
            }),
        );
    if let Some(dir) = static_dir {
        // `not_found_service` makes unmatched "/"-routes fall through to
        // the file server: "/" → index.html, assets, and 404.html for
        // genuinely unknown paths (ServeDir's own fallback).
        app = app.fallback_service(ServeDir::new(&dir).append_index_html_on_directories(true));
    }
    Ok(app)
}
