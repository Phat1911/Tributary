//! Shared harness for ledger integration tests: dual-DB, per-test isolated.
//!
//! Differs from the control/gateway harnesses in one deliberate way: the
//! Postgres half gives EVERY TEST its own database instead of sharing one.
//! Ledger invariants are global sums (trial balance, rule-7 solvency over
//! all prepaid/payable accounts) — in a shared database, concurrently
//! running tests would pollute each other's sums and the property tests
//! would flake. Isolation buys determinism; the cost is one CREATE DATABASE
//! per test (~100 ms in CI's ephemeral container).

// Each integration-test binary links this module but uses only a slice of
// the helpers — silence the per-binary "never used" noise.
#![allow(dead_code)]

use sqlx::ConnectOptions;
use sqlx::any::install_default_drivers;
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;
use tributary_common::db::{Db, run_migrations};

/// A database under test plus anything that must outlive the pool.
pub struct TestDb {
    pub name: &'static str,
    pub pool: Db,
    _keep_alive: Option<tempfile::TempDir>,
}

/// SQLite (tempfile) always; Postgres when `TRIBUTARY_TEST_DATABASE_URL` is
/// set (CI always sets it) — one FRESH database per call, fully migrated,
/// guards armed (guards ride inside `run_migrations`).
pub async fn test_dbs() -> Vec<TestDb> {
    let mut dbs = vec![sqlite_db().await];
    match std::env::var("TRIBUTARY_TEST_DATABASE_URL") {
        Ok(url) => match isolated_postgres(&url).await {
            Ok(db) => dbs.push(db),
            // PANIC, not skip — a silent skip turns a red dual-DB suite
            // green SQLite-only (same doctrine as the control harness).
            Err(err) => panic!("Postgres test DB could not be prepared: {err:#}"),
        },
        Err(_) => eprintln!("TRIBUTARY_TEST_DATABASE_URL unset — Postgres half skipped"),
    }
    dbs
}

/// Fresh migrated SQLite: real tempfile on Linux/CI (the Any driver's URL
/// form cannot express absolute Windows paths — see A-002 — so Windows falls
/// back to single-connection in-memory).
async fn sqlite_db() -> TestDb {
    #[cfg(not(windows))]
    {
        use sqlx::sqlite::SqliteConnectOptions;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("ledger-test.db");
        std::fs::File::create(&file).expect("pre-create sqlite file");
        let url = SqliteConnectOptions::new()
            .filename(&file)
            .to_url_lossy()
            .to_string();
        let pool = Db::connect(&url).await.expect("sqlite connect");
        run_migrations(&pool).await.expect("sqlite migrations");
        TestDb {
            name: "sqlite",
            pool,
            _keep_alive: Some(dir),
        }
    }
    #[cfg(windows)]
    {
        let pool = tributary_common::db::connect_sqlite_memory()
            .await
            .expect("sqlite memory");
        run_migrations(&pool).await.expect("sqlite migrations");
        TestDb {
            name: "sqlite",
            pool,
            _keep_alive: None,
        }
    }
}

/// One throwaway Postgres database per test: connect to the CI-provided
/// database, DROP DATABASE IF EXISTS + CREATE DATABASE with a unique name,
/// then connect to it and run migrations. The CI service user is the
/// container superuser, so CREATE DATABASE is always permitted; the throwaway
/// names make re-runs against a long-lived local container safe.
///
/// The databases are NOT dropped afterwards: sqlx pools close asynchronously
/// and Rust Drop is sync — dropping would race live connections. CI's
/// container is ephemeral anyway; locally, `docker rm tributary-pg-m7`
/// cleans up in one line (documented in PLAN's M7 entry).
async fn isolated_postgres(admin_url: &str) -> anyhow::Result<TestDb> {
    install_default_drivers();
    // Typed admin connection (DROP/CREATE DATABASE cannot run in a
    // transaction; `query` autocommits each statement).
    let admin =
        PgConnectOptions::from_str(admin_url).map_err(|e| anyhow::anyhow!("parse URL: {e}"))?;
    let admin_pool = sqlx::PgPool::connect_with(admin).await?;
    let db_name = format!("ledger_test_{}", tributary_common::db::new_id());
    // AssertSqlSafe: sqlx 0.9 requires dynamic SQL strings to carry an
    // explicit audit marker. These two interpolate ONLY `db_name`, minted
    // from new_id() one line above — never user input.
    sqlx::query(sqlx::AssertSqlSafe(
        format!("DROP DATABASE IF EXISTS {db_name}").as_str(),
    ))
    .execute(&admin_pool)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(
        format!("CREATE DATABASE {db_name}").as_str(),
    ))
    .execute(&admin_pool)
    .await?;
    admin_pool.close().await;

    let test_url = swap_database_in_url(admin_url, &db_name)?;
    let db = Db::connect(&test_url).await?;
    run_migrations(&db).await?;
    Ok(TestDb {
        name: "postgres",
        pool: db,
        _keep_alive: None,
    })
}

/// Replaces the path segment of a `postgres://…/name[?params]` URL. Pure
/// string surgery on an env-provided URL (both halves are operator-owned:
/// CI config + `new_id()`), so no URL-parsing dependency is warranted.
fn swap_database_in_url(url: &str, db_name: &str) -> anyhow::Result<String> {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, format!("?{q}")),
        None => (url, String::new()),
    };
    // Trim an optional trailing slash, then cut at the LAST '/' — everything
    // before it is scheme://user:pass@host:port, everything after is the
    // database name we are swapping.
    let trimmed = base.trim_end_matches('/');
    let prefix = trimmed
        .rsplit_once('/')
        .map(|(p, _)| p)
        .ok_or_else(|| anyhow::anyhow!("URL has no database path segment: {url}"))?;
    Ok(format!("{prefix}/{db_name}{query}"))
}
