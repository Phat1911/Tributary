//! Data plane: OpenAI-compatible proxy with pool-key auth and usage metering.
//!
//! M2 surface: `POST /v1/chat/completions` (streaming SSE passthrough and
//! non-streaming) and `GET /v1/models` (pool allowlist), per SPEC §7, plus
//! the merged control plane at `/api/v1` (SPEC §13 modular monolith — one
//! binary). Storage is sqlx `Any` — one portable query path over SQLite and
//! PostgreSQL.

pub mod chat;
pub mod config;
pub mod db;
pub mod error;
pub mod keys;
pub mod models_api;
pub mod quota;
pub mod routing;
pub mod sse;
pub mod state;

use axum::Router;
use axum::routing::{get, post};
use config::Config;
use state::AppState;
use tributary_common::db::Db;
use tributary_control::ControlState;

/// Builds the data-plane router over shared state.
pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/models", get(models_api::list_models))
        .with_state(state)
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
    let control_state = ControlState::new(
        db,
        http,
        config.master_key.clone(),
        config.default_models.clone(),
    );
    Ok(build_app(app_state).merge(tributary_control::router(control_state)))
}
