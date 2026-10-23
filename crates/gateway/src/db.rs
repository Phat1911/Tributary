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

/// Source IPs whose `last_seen` for this key falls inside the window —
/// the "currently active" set the D32 concurrency detector collapses into
/// shared networks. The upsert in `record_key_ip` keeps one row per
/// (key, ip), so this stays O(active sources), not O(history).
pub async fn active_ips_for_key(
    db: &Db,
    pool_key_id: &str,
    since_ms: i64,
) -> sqlx::Result<Vec<String>> {
    let sql = db.sql("SELECT ip FROM pool_key_ips WHERE pool_key_id = ? AND last_seen >= ?");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .bind(since_ms)
        .fetch_all(db.pool())
        .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("ip")).collect())
}

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

// --- Quota reservations (A-020: reserve→settle→release) ------------------------

/// One live request's reservation against a single (subject, period)
/// window — the amounts that MUST be subtracted again at settle/release.
#[derive(Debug, Clone)]
pub struct ReservedBucket {
    pub subject_kind: String,
    pub subject_id: String,
    pub period: String,
    pub window_start: i64,
    pub est_tokens: i64,
    pub est_requests: i64,
}

/// Atomically reserves `est_tokens` + `est_requests` against one window
/// (A-020). ONE conditional UPDATE is the atomicity unit on both engines:
/// PostgreSQL row-locks the bucket and re-evaluates the WHERE against the
/// committed winner (EvalPlanQual on READ COMMITTED); SQLite serializes
/// writers. Returns `false` when the window cannot absorb the estimate
/// (deny) — no partial state is left behind.
///
/// The usage aggregates inside the WHERE are CORRELATED on the bucket row
/// itself (`quota_buckets.subject_id` / `window_start`), so the check sees
/// exactly the window it reserves; their SQL must stay byte-aligned with
/// `quota.rs`'s aggregations (CAST-to-BIGINT rule, AUDIT A-006).
pub async fn reserve_bucket(
    db: &Db,
    bucket: &ReservedBucket,
    limit_tokens: Option<i64>,
    limit_requests: Option<i64>,
) -> sqlx::Result<bool> {
    // (1) Ensure the counter row exists (create-if-absent, never resets).
    let ensure = db.sql(
        "INSERT INTO quota_buckets \
         (subject_kind, subject_id, period, window_start, reserved_tokens, \
          reserved_requests, created_at) \
         VALUES (?, ?, ?, ?, 0, 0, ?) \
         ON CONFLICT (subject_kind, subject_id, period, window_start) DO NOTHING",
    );
    sqlx::query(sqlx::AssertSqlSafe(ensure.as_str()))
        .bind(&bucket.subject_kind)
        .bind(&bucket.subject_id)
        .bind(&bucket.period)
        .bind(bucket.window_start)
        .bind(now_ms())
        .execute(db.pool())
        .await?;

    // (2) The atomic conditional reservation. `? IS NULL` = "no limit of
    // this kind — condition vacuous". A NULL-bound Option<i64> is NULL on
    // both drivers (the dual-DB INTEGER rule keeps these i64-only).
    let col = crate::quota::usage_column(&bucket.subject_kind);
    let sql = db.sql(&format!(
        "UPDATE quota_buckets SET \
            reserved_tokens = reserved_tokens + ?, \
            reserved_requests = reserved_requests + ? \
         WHERE subject_kind = ? AND subject_id = ? AND period = ? \
           AND window_start = ? \
           AND (? IS NULL OR reserved_tokens + ? + \
                (SELECT CAST(COALESCE(SUM(prompt_tokens + completion_tokens), 0) AS BIGINT) \
                 FROM usage_events \
                 WHERE {col} = quota_buckets.subject_id \
                   AND ts >= quota_buckets.window_start) <= ?) \
           AND (? IS NULL OR reserved_requests + ? + \
                (SELECT COUNT(*) FROM usage_events \
                 WHERE {col} = quota_buckets.subject_id \
                   AND ts >= quota_buckets.window_start) <= ?)"
    ));
    let result = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(bucket.est_tokens)
        .bind(bucket.est_requests)
        .bind(&bucket.subject_kind)
        .bind(&bucket.subject_id)
        .bind(&bucket.period)
        .bind(bucket.window_start)
        .bind(limit_tokens)
        .bind(bucket.est_tokens)
        .bind(limit_tokens)
        .bind(limit_requests)
        .bind(bucket.est_requests)
        .bind(limit_requests)
        .execute(db.pool())
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Returns a reservation's amounts to the window (A-020 settle/release —
/// the same decrement either way: settle additionally wrote a usage row,
/// which this function deliberately does NOT touch). Floor guards make a
/// double-release harmless (bounded at zero, never negative).
pub async fn release_bucket(db: &Db, bucket: &ReservedBucket) -> sqlx::Result<u64> {
    let sql = db.sql(
        "UPDATE quota_buckets SET \
            reserved_tokens = reserved_tokens - ?, \
            reserved_requests = reserved_requests - ? \
         WHERE subject_kind = ? AND subject_id = ? AND period = ? \
           AND window_start = ? \
           AND reserved_tokens >= ? AND reserved_requests >= ?",
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(bucket.est_tokens)
        .bind(bucket.est_requests)
        .bind(&bucket.subject_kind)
        .bind(&bucket.subject_id)
        .bind(&bucket.period)
        .bind(bucket.window_start)
        .bind(bucket.est_tokens)
        .bind(bucket.est_requests)
        .execute(db.pool())
        .await?;
    Ok(result.rows_affected())
}

/// TTL sweep (A-020): drops counter rows whose window has rolled over.
/// This is the crashed-request reclaimer — a process death between
/// reserve and settle leaves the amounts pinned until the window (and
/// therefore the bucket) expires. Over-reserving is the failure mode;
/// the sweep bounds it to the window length. One portable CASE statement
/// (period lives per row); month uses a 31-day upper bound.
pub async fn sweep_reservation_windows(db: &Db, now_epoch_ms: i64) -> sqlx::Result<u64> {
    let sql = db.sql(
        "DELETE FROM quota_buckets WHERE CASE period \
            WHEN 'minute' THEN window_start + 60000 \
            WHEN 'hour' THEN window_start + 3600000 \
            WHEN 'day' THEN window_start + 86400000 \
            ELSE window_start + 2678400000 END <= ?",
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(now_epoch_ms)
        .execute(db.pool())
        .await?;
    Ok(result.rows_affected())
}
