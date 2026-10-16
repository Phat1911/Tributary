//! Database access (sqlx `Any` — one portable code path for SQLite and PostgreSQL).
//!
//! Every statement here is deliberately dialect-free (TEXT/BIGINT). sqlx's
//! Any driver does not unify placeholder syntax (`?` vs `$n`), so `Db`
//! remembers the URL scheme and `Db::sql` mechanically rewrites `?` to
//! `$1..$n` for Postgres. All SQL still lives in this one module — no
//! DB-specific SQL anywhere else (AGENTS.md convention).

use rand::RngCore;
use sqlx::AnyPool;
use sqlx::Row;
use sqlx::any::{AnyPoolOptions, install_default_drivers};
use sqlx::migrate::Migrator;
use std::time::{SystemTime, UNIX_EPOCH};

/// Embedded migrations from the repo-root `migrations/` directory.
pub static MIGRATIONS: Migrator = sqlx::migrate!("../../migrations");

/// A connected database (SQLite or PostgreSQL) behind one portable API.
#[derive(Clone)]
pub struct Db {
    pool: AnyPool,
    postgres: bool,
}

impl Db {
    /// Connect using a URL (`sqlite://…` or `postgres://…`).
    ///
    /// SQLite connections pin `PRAGMA journal_mode=WAL` and
    /// `PRAGMA busy_timeout=5000` on every pooled connection so concurrent
    /// metering writers queue briefly instead of failing `SQLITE_BUSY` —
    /// a failed insert would be a silently unmetered request (director
    /// ruling 2026-10-16). sqlx 0.9's Any driver is URL-opaque (typed
    /// `SqliteConnectOptions` and pragma URL params cannot cross it), so
    /// the pragmas run in `after_connect`, once per new connection.
    /// WAL persists in the database file; busy_timeout is per-connection
    /// and therefore genuinely needs the hook.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        install_default_drivers();
        let postgres = url.starts_with("postgres");
        let pool = if postgres {
            AnyPoolOptions::new()
                .max_connections(5)
                .connect(url)
                .await?
        } else {
            AnyPoolOptions::new()
                .max_connections(5)
                .after_connect(|conn, _| {
                    Box::pin(async move {
                        sqlx::query("PRAGMA journal_mode=WAL")
                            .fetch_all(&mut *conn)
                            .await?;
                        sqlx::query("PRAGMA busy_timeout=5000")
                            .fetch_all(&mut *conn)
                            .await?;
                        Ok(())
                    })
                })
                .connect(url)
                .await?
        };
        Ok(Self { pool, postgres })
    }

    /// The underlying pool (for migrations and test assertions).
    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    /// Placeholder translation: SQL is written with `?`; Postgres needs `$n`.
    fn sql(&self, sql: &str) -> String {
        rewrite_placeholders(sql, self.postgres)
    }
}

/// Mechanical `?` → `$1..$n` rewrite for Postgres (sqlx's Any driver does
/// not unify placeholder syntax). Only ever applied to module-owned literal
/// SQL — never to user input.
fn rewrite_placeholders(sql: &str, postgres: bool) -> String {
    if !postgres {
        return sql.to_string();
    }
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0;
    for ch in sql.chars() {
        if ch == '?' {
            n += 1;
            out.push_str(&format!("${n}"));
        } else {
            out.push(ch);
        }
    }
    out
}

/// Single-connection in-memory SQLite pool. Used only by tests on Windows
/// (cargo-interop) where absolute paths cannot be expressed through the Any
/// driver's URL form; Linux/CI tests use real tempfiles. One connection
/// means one shared in-memory database, so idle/lifetime eviction is
/// disabled to keep the schema alive for the whole test.
pub async fn connect_sqlite_memory() -> anyhow::Result<Db> {
    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect("sqlite::memory:")
        .await?;
    Ok(Db {
        pool,
        postgres: false,
    })
}

/// Run embedded migrations (idempotent — safe on every start).
pub async fn run_migrations(db: &Db) -> anyhow::Result<()> {
    MIGRATIONS.run(db.pool()).await?;
    Ok(())
}

/// Current time as unix epoch milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Random 128-bit id, hex-encoded (portable TEXT primary key).
pub fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

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

/// A metered request (subset of SPEC §6 `usage_events`; cost stays 0 until M2+).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_32_hex_chars_and_unique() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn placeholder_rewrite_only_for_postgres() {
        let sql = "INSERT INTO t (a, b) VALUES (?, ?)";
        assert_eq!(rewrite_placeholders(sql, false), sql);
        assert_eq!(
            rewrite_placeholders(sql, true),
            "INSERT INTO t (a, b) VALUES ($1, $2)"
        );
    }
}
