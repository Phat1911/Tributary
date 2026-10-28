//! Alarm tests: the ledger's failure modes must be LOUD.
//!
//! These tests create bad states with raw SQL. Like the append-only
//! burglar alarms, that is the sanctioned exception (workflow step 12) —
//! the entire point is that the PUBLIC surface cannot create these states,
//! so the only way to test detection is to simulate the catastrophe the
//! guards exist for: a tampered cache, a corrupt/bypassed entry stream, a
//! drained escrow.
//!
//! Together with `append_only.rs` these tests also document the HONEST
//! boundary of the DB-level guards: the app role holds INSERT, so a
//! compromised app can still WRITE LYING ENTRIES. The database cannot
//! know a lie from a transaction; the defenses are (1) append-only so the
//! lie can never be edited away, (2) trial balance / reconcile / solvency
//! so the lie shows up as an unbalanced or insolvent book, (3) audit.

mod harness;

use harness::test_dbs;
use tributary_common::db::Db;
use tributary_ledger::chart::{AccountCode, OwnerKind, PLATFORM_OWNER_ID};
use tributary_ledger::money::MicroUsd;
use tributary_ledger::posting::TemplateCode;
use tributary_ledger::{Ledger, PostingRequest, SqlLedger};

/// Builds a healthy book: $20 escrowed, $5 consumed, all via the trait.
async fn healthy_book(db: &Db) -> SqlLedger {
    let ledger = SqlLedger::new(db.clone());
    ledger
        .open_account(
            AccountCode::CashEscrow,
            OwnerKind::Operator,
            PLATFORM_OWNER_ID,
        )
        .await
        .unwrap();
    ledger
        .open_account(
            AccountCode::RevenueUsage,
            OwnerKind::Operator,
            PLATFORM_OWNER_ID,
        )
        .await
        .unwrap();
    ledger
        .open_account(
            AccountCode::EquityOperator,
            OwnerKind::Operator,
            PLATFORM_OWNER_ID,
        )
        .await
        .unwrap();
    ledger
        .open_account(AccountCode::PrepaidRenter, OwnerKind::Renter, "r-alarms")
        .await
        .unwrap();
    ledger
        .post(
            PostingRequest::new(TemplateCode::PrepaidFund, MicroUsd::new(20_000_000))
                .renter("r-alarms"),
        )
        .await
        .expect("fund");
    ledger
        .post(
            PostingRequest::new(TemplateCode::PrepaidConsume, MicroUsd::new(5_000_000))
                .renter("r-alarms"),
        )
        .await
        .expect("consume");
    ledger
}

#[tokio::test]
async fn tampered_balance_cache_alarms_in_reconcile() {
    for td in test_dbs().await {
        let ledger = healthy_book(&td.pool).await;
        ledger.reconcile().await.expect("healthy book reconciles");

        // The catastrophe: someone edits the CACHE (the app role CAN — the
        // cache is a derivative, not the truth; only entries are frozen).
        let sql = td.pool.sql(
            "UPDATE ledger_account_balances SET balance = balance + 1_000_000 \
             WHERE account_id = (SELECT id FROM ledger_accounts WHERE code = 'prepaid.renter')",
        );
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(td.pool.pool())
            .await
            .expect("tamper cache");

        let err = ledger.reconcile().await.expect_err("drift must alarm");
        assert!(
            err.to_string().contains("DRIFT"),
            "[{}] drift error must be loud and specific: {err}",
            td.name
        );
    }
}

#[tokio::test]
async fn unbalanced_raw_txn_alarms_in_reconcile() {
    for td in test_dbs().await {
        let ledger = healthy_book(&td.pool).await;

        // The catastrophe: a raw INSERT writes a lone debit leg (bypassing
        // posting rules entirely — what a compromised app could do).
        let sql = td.pool.sql(
            "INSERT INTO ledger_entries \
             (id, txn_id, seq, template_code, account_id, amount, direction, ref, ts) \
             VALUES ('evil', 'txn-evil', 1, 'bypass', \
                     (SELECT id FROM ledger_accounts WHERE code = 'cash.escrow'), \
                     4_000_000, 'debit', '', 1)",
        );
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(td.pool.pool())
            .await
            .expect("raw bypass insert");

        // Trial balance sees it immediately …
        assert!(
            !ledger.trial_balance().await.unwrap().balanced(),
            "[{}] unbalanced entries must unbalance the trial",
            td.name
        );
        // … and reconcile names it as a High-priority corruption.
        let err = ledger.reconcile().await.expect_err("corruption must alarm");
        assert!(
            err.to_string().contains("TRIAL BALANCE VIOLATION"),
            "[{}] wrong alarm: {err}",
            td.name
        );
    }
}

#[tokio::test]
async fn escrow_drain_alarms_loudly_in_the_solvency_check() {
    for td in test_dbs().await {
        let ledger = healthy_book(&td.pool).await;

        // State before the crime: escrow 20, prepaid 15 → headroom 5.
        let before = ledger.solvency_report().await.unwrap();
        assert_eq!(before.escrow, MicroUsd::new(20_000_000));
        assert_eq!(before.prepaid, MicroUsd::new(15_000_000));
        assert_eq!(before.headroom, MicroUsd::new(5_000_000));

        // The catastrophe: a BALANCED raw txn "drains" escrow — e.g. the
        // operator siphoning cash against their own equity claim. Balanced
        // books, so the trial-balance alarm stays quiet; only rule 7 sees
        // that custodial cash no longer covers custodial debt.
        let sql = td.pool.sql(
            "INSERT INTO ledger_entries \
             (id, txn_id, seq, template_code, account_id, amount, direction, ref, ts) \
             VALUES \
             ('evil-1', 'txn-drain', 1, 'bypass', \
              (SELECT id FROM ledger_accounts WHERE code = 'cash.escrow'), \
              10_000_000, 'credit', '', 1), \
             ('evil-2', 'txn-drain', 2, 'bypass', \
              (SELECT id FROM ledger_accounts WHERE code = 'equity.operator'), \
              10_000_000, 'debit', '', 1)",
        );
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(td.pool.pool())
            .await
            .expect("drain insert");

        // escrow 10 < prepaid 15 → violation of $5, loud, with the numbers.
        let err = ledger
            .solvency_check()
            .await
            .expect_err("drained escrow must violate rule 7");
        assert!(
            err.to_string().contains("SOLVENCY VIOLATION"),
            "[{}] wrong alarm: {err}",
            td.name
        );
        assert!(
            err.to_string().contains("10"),
            "[{}] escrow number in message",
            td.name
        );
        // The report (no judgment) still returns the ugly truth quietly —
        // dashboards may display it; only the CHECK escalates.
        let report = ledger.solvency_report().await.unwrap();
        assert_eq!(report.escrow, MicroUsd::new(10_000_000));
        assert_eq!(report.headroom, MicroUsd::new(-5_000_000));
    }
}
