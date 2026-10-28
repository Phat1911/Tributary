//! Property tests for the ledger invariants (SPEC M7 done-when):
//!   * every transaction balances (Σdebits = Σcredits per txn_id)
//!   * the global trial balance closes
//!   * cached balances reconstruct from entries (shadow model + reconcile)
//!   * normal balances: no account sits on its wrong side
//!   * rule 7: template-driven flows can NEVER violate solvency
//!   * the DB chart seed and the code enum never drift
//!
//! Randomness: a tiny seeded LCG (Lehmer-style), NOT the `rand` crate —
//! property tests want REPRODUCIBILITY most of all; a fixed seed means a
//! failure on CI re-runs identically locally.

mod harness;

use harness::test_dbs;
use std::collections::HashMap;
use std::str::FromStr;
use tributary_common::db::Db;
use tributary_ledger::chart::{
    AccountCode, AccountType, Direction, OwnerKind, PLATFORM_OWNER_ID, parse_counterparties,
};
use tributary_ledger::posting::{PartySlot, TemplateCode};
use tributary_ledger::{Ledger, MicroUsd, PostingRequest, SqlLedger};

/// Deterministic pseudo-random generator (a 64-bit LCG). Enough entropy for
/// a test walk; reproducible by construction.
struct Lcg(u64);

impl Lcg {
    /// Knuth-recommended constants; `>> 33` takes the high-entropy bits.
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// A value in `0..n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The shadow model: what the test BELIEVES each account's signed balance
/// (Σdebits − Σcredits) should be, updated only from accepted receipts.
/// Keyed by (chart code, owner id).
type Shadow = HashMap<(AccountCode, String), i64>;

/// One random walk: opens accounts, fires random template postings, then
/// checks every invariant against the database.
async fn walk_and_verify(db: &Db, seed: u64, iterations: u32) {
    let ledger = SqlLedger::new(db.clone());
    let mut rng = Lcg(seed);

    // Platform accounts first (every template touches at least one).
    for code in [
        AccountCode::CashEscrow,
        AccountCode::RevenueUsage,
        AccountCode::EquityOperator,
    ] {
        ledger
            .open_account(code, OwnerKind::Operator, PLATFORM_OWNER_ID)
            .await
            .expect("open platform account");
    }
    let renters: Vec<&str> = vec!["r0", "r1", "r2", "r3"];
    let depositors: Vec<&str> = vec!["d0", "d1", "d2"];
    for renter in &renters {
        ledger
            .open_account(AccountCode::PrepaidRenter, OwnerKind::Renter, renter)
            .await
            .expect("open renter account");
    }
    for depositor in &depositors {
        for code in [
            AccountCode::CapacityContributed,
            AccountCode::ClaimsContributed,
            AccountCode::PayableDepositor,
        ] {
            ledger
                .open_account(code, OwnerKind::Depositor, depositor)
                .await
                .expect("open depositor account");
        }
    }

    let mut shadow = Shadow::new();
    let all_templates = [
        TemplateCode::PrepaidFund,
        TemplateCode::PrepaidConsume,
        TemplateCode::SettleFee,
        TemplateCode::SettleDistribute,
        TemplateCode::DepositCapacity,
        TemplateCode::WithdrawPayable,
    ];

    for i in 0..iterations {
        let template = all_templates[rng.below(all_templates.len() as u64) as usize];
        let renter = renters[rng.below(renters.len() as u64) as usize];
        let depositor = depositors[rng.below(depositors.len() as u64) as usize];
        let amount = MicroUsd::new(1 + rng.below(500) as i64);
        let mut request = PostingRequest::new(template, amount);
        let definition = template.definition();
        for leg in definition.legs {
            request = match leg.party {
                PartySlot::Renter => request.renter(renter),
                PartySlot::Depositor => request.depositor(depositor),
                PartySlot::Platform => request,
            };
        }
        match ledger.post(request).await {
            Ok(receipt) => {
                assert_eq!(receipt.amount, amount);
                // Shadow-update: every leg of the template moves its account.
                for leg in definition.legs {
                    let owner = match leg.party {
                        PartySlot::Platform => PLATFORM_OWNER_ID.to_string(),
                        PartySlot::Renter => renter.to_string(),
                        PartySlot::Depositor => depositor.to_string(),
                    };
                    let key = (leg.account, owner);
                    let delta = match leg.direction {
                        Direction::Debit => amount.micros(),
                        Direction::Credit => -amount.micros(),
                    };
                    *shadow.entry(key).or_insert(0) += delta;
                }
            }
            Err(err) => {
                // The only legal refusal is sufficiency — anything else is a
                // bug (owner-slot mistakes would mean the walk built the
                // request wrong).
                assert!(
                    err.to_string().contains("insufficient"),
                    "iteration {i} (seed {seed}): unexpected refusal: {err}"
                );
            }
        }
    }

    // --- invariant: cached balances reconstruct from entries ------------
    // (a) against the test's own shadow model …
    for ((code, owner), expected_signed) in &shadow {
        let id = ledger
            .account_id(*code, owner_kind_for(*code), owner)
            .await
            .expect("account id")
            .expect("account was opened");
        let balance = ledger.account_balance(&id).await.expect("balance");
        assert_eq!(
            balance.signed.micros(),
            *expected_signed,
            "cache != shadow model for {code:?}/{owner}"
        );
        // (b) …and the normal side is non-negative (no wrong-side accounts).
        assert!(
            balance.normal.micros() >= 0,
            "{code:?}/{owner} sat on its wrong side: {}",
            balance.normal
        );
    }
    // (c) …and against the ledger's own reconciler (cache vs entries).
    ledger.reconcile().await.expect("reconcile: cache == truth");

    // --- invariant: every transaction balances ---------------------------
    let sql = db.sql(
        "SELECT txn_id, \
                CAST(COALESCE(SUM(CASE WHEN direction = 'debit'  THEN amount ELSE 0 END), 0) AS BIGINT) AS d, \
                CAST(COALESCE(SUM(CASE WHEN direction = 'credit' THEN amount ELSE 0 END), 0) AS BIGINT) AS c \
         FROM ledger_entries GROUP BY txn_id",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .fetch_all(db.pool())
        .await
        .expect("per-txn sums");
    use sqlx::Row;
    for row in &rows {
        let txn: String = row.try_get("txn_id").unwrap();
        let d: i64 = row.try_get("d").unwrap();
        let c: i64 = row.try_get("c").unwrap();
        assert_eq!(d, c, "txn {txn} does not balance");
    }

    // --- invariant: the trial balance closes ------------------------------
    let trial = ledger.trial_balance().await.expect("trial balance");
    assert!(trial.balanced(), "trial balance unclosed: {trial:?}");

    // --- invariant: rule 7 can hold only within template discipline -------
    // A pure template walk can never drive cash:escrow below its custodial
    // debts — this assert is the whole point of posting rules.
    ledger
        .solvency_check()
        .await
        .expect("template-driven flows must never violate rule 7");
}

/// Maps a chart code to the owner kind its accounts carry (platform codes
/// are operator-owned in M7).
fn owner_kind_for(code: AccountCode) -> OwnerKind {
    match code {
        AccountCode::PrepaidRenter => OwnerKind::Renter,
        AccountCode::CapacityContributed
        | AccountCode::ClaimsContributed
        | AccountCode::PayableDepositor => OwnerKind::Depositor,
        _ => OwnerKind::Operator,
    }
}

#[tokio::test]
async fn random_template_walks_preserve_every_invariant() {
    // test_dbs() is called PER SEED: every walk needs a database with no
    // history, and the shadow model starts empty. Reusing one database
    // across seeds would accumulate entries the new shadow never counted
    // (found the hard way — the ledger was right, the test was stateful).
    // Several fixed seeds: different explorations, all reproducible.
    for seed in [7_u64, 2026_1028, 42] {
        for td in test_dbs().await {
            walk_and_verify(&td.pool, seed, 150).await;
        }
    }
}

/// The DB chart seed must equal the code enum's SEED — codes, types, and
/// counterparties exactly (definitions are prose; they must merely exist).
#[tokio::test]
async fn chart_seed_in_db_matches_the_code_enum() {
    for td in test_dbs().await {
        let sql = td.pool.sql(
            "SELECT code, account_type, allowed_counterparties, definition FROM chart_of_accounts",
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_all(td.pool.pool())
            .await
            .expect("read chart");
        assert_eq!(
            rows.len(),
            tributary_ledger::chart::SEED.len(),
            "[{}] chart row count drifted from the code seed",
            td.name
        );
        use sqlx::Row;
        for row in &rows {
            let code: String = row.try_get("code").unwrap();
            let type_str: String = row.try_get("account_type").unwrap();
            let counterparties: String = row.try_get("allowed_counterparties").unwrap();
            let definition: String = row.try_get("definition").unwrap();
            let code_enum = AccountCode::from_str(&code)
                .unwrap_or_else(|e| panic!("[{}] unknown chart code in DB: {e}", td.name));
            let type_enum = AccountType::from_str(&type_str)
                .unwrap_or_else(|e| panic!("[{}] unknown account type in DB: {e}", td.name));
            let seed_row = tributary_ledger::chart::row(code_enum);
            assert_eq!(
                type_enum, seed_row.account_type,
                "[{}] {code}: type drift between DB seed and code",
                td.name
            );
            let db_counterparties = parse_counterparties(&counterparties)
                .unwrap_or_else(|e| panic!("[{}] {code}: bad counterparties column: {e}", td.name));
            assert_eq!(
                db_counterparties, seed_row.allowed_counterparties,
                "[{}] {code}: counterparty drift between DB seed and code",
                td.name
            );
            assert!(
                !definition.trim().is_empty(),
                "[{}] {code}: empty definition",
                td.name
            );
        }
    }
}
