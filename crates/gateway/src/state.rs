//! Shared application state.

use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use crate::quota::QuotaEngine;

/// Cloneable handle shared by all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub http: reqwest::Client,
    pub config: Arc<Config>,
    /// Rule-4 gate: quotas/rate limits checked before any upstream call.
    pub quota: QuotaEngine,
}

impl AppState {
    /// Assembles state (one engine + client per process).
    pub fn new(db: Db, http: reqwest::Client, config: Config) -> Self {
        Self {
            db,
            http,
            config: Arc::new(config),
            quota: QuotaEngine::new(),
        }
    }
}
