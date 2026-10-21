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
    /// 429/529 cooldown per org for classifier scoring (fail-open +
    /// backoff, director research 2026-10-21).
    pub classifier_backoff: ClassifierBackoff,
}

/// Per-org cooldown after the classifier endpoint answered 429/529
/// (director research 2026-10-21: fail-open + backoff). In-process on
/// purpose: the cost of a wrong decision is a few skipped scores, so
/// durability across restarts buys nothing.
#[derive(Default, Clone)]
pub struct ClassifierBackoff {
    // Arc so AppState stays Clone (the QuotaEngine/OrgSettingsCache
    // pattern): shared process-wide state behind one allocation.
    inner: std::sync::Arc<std::sync::Mutex<HashMap<String, std::time::Instant>>>,
}

impl ClassifierBackoff {
    /// How long the org still sits in cooldown (zero = clear).
    pub fn remaining(&self, org_id: &str) -> std::time::Duration {
        self.inner
            .lock()
            .unwrap()
            .get(org_id)
            .map(|until| until.saturating_duration_since(std::time::Instant::now()))
            .unwrap_or_default()
    }

    /// Enters the cooldown after a 429/529 (rate/capacity — retrying
    /// immediately would just hammer a struggling vendor).
    pub fn trip(&self, org_id: &str) {
        self.inner.lock().unwrap().insert(
            org_id.to_string(),
            std::time::Instant::now() + std::time::Duration::from_secs(60),
        );
    }
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
            classifier_backoff: ClassifierBackoff::default(),
            db,
            http,
            config: Arc::new(config),
            quota: QuotaEngine::new(),
            geo,
        }
    }
}
