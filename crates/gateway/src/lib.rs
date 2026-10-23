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
    Ok(build_app(app_state).merge(tributary_control::router(control_state)))
}
