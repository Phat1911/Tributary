//! Shared application state.

use std::collections::HashMap;
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
    /// D9 in-proc cache for org detector settings (D32): org_id →
    /// (fetched_at, window). Same 5 s TTL discipline as the quota rules
    /// cache — rare-change data, per-request reads.
    pub org_settings: OrgSettingsCache,
}

/// Tiny TTL cache over `org_settings` rows (D9 layering: Postgres +
/// in-process cache). Falls back to the compiled default on any error —
/// an unreadable knob must never take the request path down.
#[derive(Default, Clone)]
pub struct OrgSettingsCache {
    // Arc so AppState stays Clone (the QuotaEngine pattern): the cache is
    // process-wide state behind one allocation, cloned handles share it.
    inner: std::sync::Arc<std::sync::Mutex<HashMap<String, (std::time::Instant, i64)>>>,
}

impl OrgSettingsCache {
    const TTL_SECS: u64 = 5;

    /// The concurrency window for this org, cached for 5 s.
    pub async fn concurrency_window_secs(&self, db: &Db, org_id: &str) -> i64 {
        if let Some((fetched_at, window)) = self.inner.lock().unwrap().get(org_id)
            && fetched_at.elapsed().as_secs() < Self::TTL_SECS
        {
            return *window;
        }
        let window = tributary_control::db::org_settings(db, org_id)
            .await
            .map(|s| s.concurrency_window_secs)
            .unwrap_or(tributary_control::db::DEFAULT_CONCURRENCY_WINDOW_SECS);
        self.inner
            .lock()
            .unwrap()
            .insert(org_id.to_string(), (std::time::Instant::now(), window));
        window
    }
}

impl AppState {
    /// Assembles state (one engine + client per process).
    pub fn new(db: Db, http: reqwest::Client, config: Config) -> Self {
        let geo = crate::geo::build_geo_resolver(config.geoip_db_path.as_deref());
        Self {
            org_settings: OrgSettingsCache::default(),
            db,
            http,
            config: Arc::new(config),
            quota: QuotaEngine::new(),
            geo,
        }
    }
}
