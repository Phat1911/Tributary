//! Gateway-owned queries over the shared portable adapter.
//!
//! The adapter itself (sqlx `Any`, placeholder rewrite, WAL/busy_timeout
//! pins, migrations) lives in `tributary_common::db` — moved there in M2
//! when the control plane began sharing it (M1 PLAN note). This module
//! keeps only data-plane queries: pool-key lookup and usage metering.

use sqlx::Row;

// Re-exported so existing `tributary_gateway::db::*` call sites (tests,
// binary subcommands) keep working unchanged after the M2 move to common.
pub use tributary_common::db::{
    Db, MIGRATIONS, connect_sqlite_memory, new_id, now_ms, run_migrations,
};

/// A `pool_keys` row (M2: carries its pool binding for routing).
#[derive(Debug, Clone)]
pub struct PoolKeyRow {
    pub id: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub kind: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub user_id: Option<String>,
    /// The pool this key unlocks; NULL only for pre-M2 rows (rejected).
    pub pool_id: Option<String>,
}

/// Fetch a pool key by its storage hash (constant-time verify after fetch).
pub async fn lookup_pool_key(db: &Db, key_hash: &str) -> sqlx::Result<Option<PoolKeyRow>> {
    let sql = db.sql(
        "SELECT id, key_hash, key_prefix, kind, status, expires_at, user_id, pool_id \
         FROM pool_keys WHERE key_hash = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(key_hash)
        .fetch_optional(db.pool())
        .await?
        .map(|row| PoolKeyRow {
            id: row.get("id"),
            key_hash: row.get("key_hash"),
            key_prefix: row.get("key_prefix"),
            kind: row.get("kind"),
            status: row.get("status"),
            expires_at: row.get("expires_at"),
            user_id: row.get("user_id"),
            pool_id: row.get("pool_id"),
        });
    Ok(row)
}

/// A new pool key to insert (raw key material must never reach this layer).
#[derive(Debug, Clone)]
pub struct NewPoolKey {
    pub id: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub kind: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub user_id: Option<String>,
    pub pool_id: Option<String>,
    pub created_at: i64,
}

/// Persist a new pool key row.
pub async fn insert_pool_key(db: &Db, key: &NewPoolKey) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO pool_keys \
         (id, key_hash, key_prefix, kind, status, expires_at, user_id, pool_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&key.id)
        .bind(&key.key_hash)
        .bind(&key.key_prefix)
        .bind(&key.kind)
        .bind(&key.status)
        .bind(key.expires_at)
        .bind(key.user_id.as_deref())
        .bind(key.pool_id.as_deref())
        .bind(key.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// A metered request (subset of SPEC §6 `usage_events`; cost stays 0 until
/// D23). M2 adds the pool + provider-key attribution columns.
#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub id: String,
    pub ts: i64,
    pub pool_key_id: String,
    pub user_id: Option<String>,
    pub pool_id: Option<String>,
    pub provider_key_id: Option<String>,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micros: i64,
    pub latency_ms: i64,
    pub status: String,
}

/// Append one usage event (the metering truth quota checks aggregate over).
pub async fn insert_usage_event(db: &Db, event: &UsageEvent) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO usage_events \
         (id, ts, pool_key_id, user_id, pool_id, provider_key_id, model, \
          prompt_tokens, completion_tokens, cost_micros, latency_ms, status) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&event.id)
        .bind(event.ts)
        .bind(&event.pool_key_id)
        .bind(event.user_id.as_deref())
        .bind(event.pool_id.as_deref())
        .bind(event.provider_key_id.as_deref())
        .bind(&event.model)
        .bind(event.prompt_tokens)
        .bind(event.completion_tokens)
        .bind(event.cost_micros)
        .bind(event.latency_ms)
        .bind(&event.status)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// A `usage_events` row as read back (for tests and later dashboards).
#[derive(Debug, Clone)]
pub struct UsageEventRow {
    pub id: String,
    pub ts: i64,
    pub pool_key_id: String,
    pub user_id: Option<String>,
    pub pool_id: Option<String>,
    pub provider_key_id: Option<String>,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micros: i64,
    pub latency_ms: i64,
    pub status: String,
}

/// All usage events for one pool key, oldest first.
pub async fn usage_events_for_key(db: &Db, pool_key_id: &str) -> sqlx::Result<Vec<UsageEventRow>> {
    let sql = db.sql(
        "SELECT id, ts, pool_key_id, user_id, pool_id, provider_key_id, model, \
          prompt_tokens, completion_tokens, cost_micros, latency_ms, status \
         FROM usage_events WHERE pool_key_id = ? ORDER BY ts",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| UsageEventRow {
            id: row.get("id"),
            ts: row.get("ts"),
            pool_key_id: row.get("pool_key_id"),
            user_id: row.get("user_id"),
            pool_id: row.get("pool_id"),
            provider_key_id: row.get("provider_key_id"),
            model: row.get("model"),
            prompt_tokens: row.get("prompt_tokens"),
            completion_tokens: row.get("completion_tokens"),
            cost_micros: row.get("cost_micros"),
            latency_ms: row.get("latency_ms"),
            status: row.get("status"),
        })
        .collect();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// M4: leak prevention — IP allowlists, per-key IP history
// ---------------------------------------------------------------------------

/// The CIDR strings pinned to one key (empty = unrestricted/dormant).
/// Raw strings suffice — the caller parses once per request for matching.
pub async fn ip_allowlist_cidrs(db: &Db, pool_key_id: &str) -> sqlx::Result<Vec<String>> {
    let sql = db.sql("SELECT cidr FROM ip_allowlists WHERE pool_key_id = ?");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .fetch_all(db.pool())
        .await?;
    Ok(rows.iter().map(|r| r.get("cidr")).collect())
}

/// Records one (key, source-ip) sighting, upserting `last_seen` and
/// refreshing `country` when the resolver knows it. `ON CONFLICT … DO
/// UPDATE` is the one upsert form SQLite ≥3.24 and PostgreSQL share, so it
/// crosses the Any adapter unchanged.
pub async fn record_key_ip(
    db: &Db,
    pool_key_id: &str,
    ip: &str,
    country: Option<&str>,
    now: i64,
) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO pool_key_ips (pool_key_id, ip, country, first_seen, last_seen) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (pool_key_id, ip) DO UPDATE \
         SET last_seen = excluded.last_seen, \
             country = COALESCE(excluded.country, pool_key_ips.country)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .bind(ip)
        .bind(country)
        .bind(now)
        .bind(now)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// The DISTINCT countries a key has been seen from, excluding the current
/// sighting's IP (the travel check asks "is this NEW relative to history?").
pub async fn prior_countries_for_key(
    db: &Db,
    pool_key_id: &str,
    exclude_ip: &str,
) -> sqlx::Result<Vec<String>> {
    let sql = db.sql(
        "SELECT DISTINCT country FROM pool_key_ips \
         WHERE pool_key_id = ? AND country IS NOT NULL AND ip != ?",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .bind(exclude_ip)
        .fetch_all(db.pool())
        .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("country")).collect())
}
