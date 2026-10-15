//! Data plane: OpenAI-compatible proxy with pool-key auth and usage metering.
//!
//! M1 surface: `POST /v1/chat/completions` (streaming SSE passthrough and
//! non-streaming) and `GET /v1/models` (static config list), per SPEC §7.
//! Storage is sqlx `Any` — one portable query path over SQLite and PostgreSQL.

pub mod chat;
pub mod config;
pub mod db;
pub mod error;
pub mod keys;
pub mod models_api;
pub mod sse;
pub mod state;

use axum::Router;
use axum::routing::{get, post};
use config::Config;
use state::AppState;

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
