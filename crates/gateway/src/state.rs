//! Shared application state.

use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;

/// Cloneable handle shared by all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub http: reqwest::Client,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(db: Db, http: reqwest::Client, config: Config) -> Self {
        Self {
            db,
            http,
            config: Arc::new(config),
        }
    }
}
