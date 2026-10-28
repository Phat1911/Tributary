//! Burglar-alarm tests (rule 3): direct UPDATE and direct DELETE on
//! `ledger_entries` must FAIL on both engines, executed as the APP's
//! connection — every CI run, forever.
//!
//! These tests seed rows with raw SQL. That is the ONE sanctioned exception
//! to the journey-proof rule (workflow step 12): their entire job is proving
//! raw SQL loses. Everywhere else, entries exist only via posting rules.
//!
//! What "app connection" means per engine:
//!   * PostgreSQL: the migrations run as the table OWNER, and a separate
//!     non-owner role `tributary_app` holds only SELECT + INSERT. The test
//!     executes `SET ROLE tributary_app` and attacks from there — exactly
//!     the privileges a production app connection would have.
//!   * SQLite: there are no roles; the guard is BEFORE UPDATE/DELETE
//!     triggers that RAISE(ABORT, 'ledger_entries is append-only').
//!
//! The alarm must be PRECISE: INSERT (the legal append) still succeeds as
//! the app role while UPDATE/DELETE are refused. A guard that blocks
//! everything would be just as broken as one that blocks nothing.

mod harness;

use harness::test_dbs;
use sqlx::AnyConnection;
use tributary_common::db::Db;

/// Seeds one account + two entries as the OWNER (migration-level trust):
/// these rows exist only to be attacked below.
async fn seed_attack_surface(db: &Db) {
    let acct = db.sql(
        "INSERT INTO ledger_accounts (id, code, owner_kind, owner_id, currency, created_at) \
         VALUES ('acct-1', 'cash.escrow', 'operator', 'platform', 'USD', 1)",
    );
    sqlx::query(sqlx::AssertSqlSafe(acct.as_str()))
        .execute(db.pool())
        .await
        .expect("seed account");
    for (id, dir) in [("e-update", "debit"), ("e-delete", "credit")] {
        let entry = db.sql(
            "INSERT INTO ledger_entries \
             (id, txn_id, seq, template_code, account_id, amount, direction, ref, ts) \
             VALUES (?, 'txn-seed', 1, 'prepaid.fund', 'acct-1', 1000, ?, '', 1)",
        );
        sqlx::query(sqlx::AssertSqlSafe(entry.as_str()))
            .bind(id)
            .bind(dir)
            .execute(db.pool())
            .await
            .expect("seed entry");
    }
}

/// Runs one statement on the connection, capturing success/failure without
/// panicking — outcomes are asserted AFTER `RESET ROLE` so a failed attack
/// can never leak a role-restricted connection back into the pool.
async fn attempt(db: &Db, conn: &mut AnyConnection, sql: &str) -> Result<(), String> {
    let rewritten = db.sql(sql);
    match sqlx::query(sqlx::AssertSqlSafe(rewritten.as_str()))
        .execute(&mut *conn)
        .await
    {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{err}")),
    }
}

#[tokio::test]
async fn direct_update_and_delete_on_ledger_entries_must_fail() {
    for td in test_dbs().await {
        let db = &td.pool;
        seed_attack_surface(db).await;

        // Acquire ONE connection and pin the app role onto it (Postgres).
        // All attack statements run on this same connection.
        let mut conn = db.pool().acquire().await.expect("acquire");
        if db.postgres() {
            sqlx::query("SET ROLE tributary_app")
                .execute(&mut *conn)
                .await
                .expect("SET ROLE — tributary_app must exist (guards armed)");
        }

        // The legal append: still works. The alarm must not brick the door.
        let legal_insert = attempt(
            db,
            &mut conn,
            "INSERT INTO ledger_entries \
             (id, txn_id, seq, template_code, account_id, amount, direction, ref, ts) \
             VALUES ('e-legal', 'txn-seed', 2, 'prepaid.fund', 'acct-1', 500, 'debit', '', 2)",
        )
        .await;
        // The attacks: rewriting history, and erasing it.
        let attack_update = attempt(
            db,
            &mut conn,
            "UPDATE ledger_entries SET amount = 999999 WHERE id = 'e-update'",
        )
        .await;
        let attack_delete = attempt(
            db,
            &mut conn,
            "DELETE FROM ledger_entries WHERE id = 'e-delete'",
        )
        .await;

        // Always restore owner identity BEFORE asserting (Postgres): a panic
        // while role-pinned would hand the pool a degraded connection.
        if db.postgres() {
            sqlx::query("RESET ROLE")
                .execute(&mut *conn)
                .await
                .expect("RESET ROLE");
        }
        drop(conn);

        // --- the alarm itself -------------------------------------------------
        assert!(
            legal_insert.is_ok(),
            "[{}] INSERT as app role must still work: {:?}",
            td.name,
            legal_insert.err()
        );
        for (attack, name) in [(attack_update, "UPDATE"), (attack_delete, "DELETE")] {
            let err = attack.err().unwrap_or_else(|| {
                panic!(
                    "[{}] direct {name} on ledger_entries SUCCEEDED — history is \
                     mutable, rule 3 is broken",
                    td.name
                )
            });
            // And it must fail for the RIGHT reason: our guard, not an
            // accident (typo'd table, dropped connection, ...).
            let expected = if db.postgres() {
                "permission denied"
            } else {
                "append-only"
            };
            assert!(
                err.contains(expected),
                "[{}] {name} failed for the wrong reason (wanted '{expected}'): {err}",
                td.name
            );
        }

        // History is provably intact: both victims still carry their seed.
        let survivors = db.sql(
            "SELECT COUNT(*) AS n FROM ledger_entries \
             WHERE id IN ('e-update', 'e-delete') AND amount = 1000",
        );
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(survivors.as_str()))
            .fetch_one(db.pool())
            .await
            .expect("count survivors");
        assert_eq!(
            n, 2,
            "[{}] seeded entries must survive the attacks",
            td.name
        );
    }
}
