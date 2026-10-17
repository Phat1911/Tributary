//! Portable database access (sqlx `Any` — one code path for SQLite and PostgreSQL).
//!
//! This module moved from `crates/gateway/src/db.rs` in M2 when the control
//! plane started sharing queries (M1 PLAN note: "revisit placement when the M2
//! control plane shares queries"). Per the AGENTS.md DB convention, the only
//! DB-specific SQL in the repo lives behind [`Db::sql`]: sqlx's Any driver
//! does not unify placeholder syntax (`?` vs `$n`), so SQL is written with
//! `?` and mechanically rewritten for Postgres here.

use rand::RngCore;
use sqlx::AnyPool;
use sqlx::any::{AnyPoolOptions, install_default_drivers};
use sqlx::migrate::Migrator;
use std::time::{SystemTime, UNIX_EPOCH};

/// Embedded migrations from the repo-root `migrations/` directory.
///
/// `sqlx::migrate!` is a proc-macro: it reads the directory at COMPILE time
/// and embeds the SQL into the binary, so releases never depend on the repo
/// checkout being present at runtime.
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
                    // Box::pin makes the returned future heap-allocated and
                    // 'static, which the connection hook's signature requires.
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
    pub fn sql(&self, sql: &str) -> String {
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
