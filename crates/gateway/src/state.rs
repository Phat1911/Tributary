//! Shared application state.

use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use crate::geo::CountryResolver;
use crate::quota::QuotaEngine;

/// Cloneable handle shared by all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub http: reqwest::Client,
    pub config: Arc<Config>,
    /// Rule-4 gate: quotas/rate limits checked before any upstream call.
    pub quota: QuotaEngine,
    /// Country resolver for the (dormant-by-default) impossible-travel
    /// check — `NoGeo` unless a GeoLite2 db path + `geoip` feature are
    /// both present. One Arc per process, read per request.
    pub geo: Arc<dyn CountryResolver>,
}

impl AppState {
    /// Assembles state (one engine + client per process).
    pub fn new(db: Db, http: reqwest::Client, config: Config) -> Self {
        let geo = crate::geo::build_geo_resolver(config.geoip_db_path.as_deref());
        Self {
            db,
            http,
            config: Arc::new(config),
            quota: QuotaEngine::new(),
            geo,
        }
    }
}
