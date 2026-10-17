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

/// A `pool_keys` row (subset per SPEC §6 / M1 kickoff).
#[derive(Debug, Clone)]
pub struct PoolKeyRow {
    pub id: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub kind: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub user_id: Option<String>,
}

/// Fetch a pool key by its storage hash (constant-time verify after fetch).
pub async fn lookup_pool_key(db: &Db, key_hash: &str) -> sqlx::Result<Option<PoolKeyRow>> {
    let sql = db.sql(
        "SELECT id, key_hash, key_prefix, kind, status, expires_at, user_id \
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
    pub created_at: i64,
}

/// Persist a new pool key row.
pub async fn insert_pool_key(db: &Db, key: &NewPoolKey) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO pool_keys \
         (id, key_hash, key_prefix, kind, status, expires_at, user_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&key.id)
        .bind(&key.key_hash)
        .bind(&key.key_prefix)
        .bind(&key.kind)
        .bind(&key.status)
        .bind(key.expires_at)
        .bind(key.user_id.as_deref())
        .bind(key.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// A metered request (subset of SPEC §6 `usage_events`; cost stays 0 until D23).
#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub id: String,
    pub ts: i64,
    pub pool_key_id: String,
    pub user_id: Option<String>,
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
         (id, ts, pool_key_id, user_id, model, prompt_tokens, completion_tokens, \
          cost_micros, latency_ms, status) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&event.id)
        .bind(event.ts)
        .bind(&event.pool_key_id)
        .bind(event.user_id.as_deref())
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
        "SELECT id, ts, pool_key_id, user_id, model, prompt_tokens, completion_tokens, \
          cost_micros, latency_ms, status \
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
