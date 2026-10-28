//! Journey proof (workflow step 12): the §11 economic story walks the
//! PUBLIC `LedgerTrait` surface ONLY — no test seeds ledger rows by raw
//! SQL. If the product cannot do it through the trait, the test cannot
//! either.
//!
//! The story (SPEC §11, all against a dark ledger with no real money):
//!   renter prepays → consumes → depositor contributes capacity →
//!   settlement splits revenue into fee + distributable → depositor
//!   withdraws → solvency holds throughout → overdraw is refused.

mod harness;

use harness::test_dbs;
use tributary_common::db::Db;
use tributary_ledger::chart::{AccountCode, OwnerKind, PLATFORM_OWNER_ID};
use tributary_ledger::money::MicroUsd;
use tributary_ledger::posting::TemplateCode;
use tributary_ledger::{Ledger, PostingRequest, SqlLedger};

/// Dollars → MicroUsd for readable amounts (integer math only).
const fn usd(whole: i64, micros: i64) -> MicroUsd {
    MicroUsd::new(whole * 1_000_000 + micros)
}

#[tokio::test]
async fn the_root_pool_economic_story_writes_only_through_the_trait() {
    for td in test_dbs().await {
        let db: &Db = &td.pool;
        let ledger = SqlLedger::new(db.clone());

        // --- platform accounts (the operator's own books) ----------------
        let escrow = ledger
            .open_account(
                AccountCode::CashEscrow,
                OwnerKind::Operator,
                PLATFORM_OWNER_ID,
            )
            .await
            .expect("open escrow");
        let revenue = ledger
            .open_account(
                AccountCode::RevenueUsage,
                OwnerKind::Operator,
                PLATFORM_OWNER_ID,
            )
            .await
            .expect("open revenue");
        let equity = ledger
            .open_account(
                AccountCode::EquityOperator,
                OwnerKind::Operator,
                PLATFORM_OWNER_ID,
            )
            .await
            .expect("open equity");

        // --- party accounts: onboarding opens them BEFORE money moves ------
        // (post() refuses unknown accounts — AccountNotFound — so there is
        // no path where a typo'd owner id silently mints a new account.)
        ledger
            .open_account(AccountCode::PrepaidRenter, OwnerKind::Renter, "renter-1")
            .await
            .expect("open renter account");
        for code in [
            AccountCode::CapacityContributed,
            AccountCode::ClaimsContributed,
            AccountCode::PayableDepositor,
        ] {
            ledger
                .open_account(code, OwnerKind::Depositor, "depositor-1")
                .await
                .expect("open depositor account");
        }

        // --- 1. renter prepays $10 ---------------------------------------
        ledger
            .post(PostingRequest::new(TemplateCode::PrepaidFund, usd(10, 0)).renter("renter-1"))
            .await
            .expect("prepaid.fund");
        assert_eq!(
            ledger
                .normal_side_by_code(AccountCode::PrepaidRenter)
                .await
                .unwrap(),
            usd(10, 0),
            "[{}] renter prepaid must be a liability of exactly what was paid",
            td.name
        );
        assert_eq!(
            ledger.account_balance(&escrow).await.unwrap().normal,
            usd(10, 0),
            "[{}] escrow must hold the prepaid cash",
            td.name
        );

        // --- 2. metered usage consumes $3 ---------------------------------
        ledger
            .post(PostingRequest::new(TemplateCode::PrepaidConsume, usd(3, 0)).renter("renter-1"))
            .await
            .expect("prepaid.consume");
        assert_eq!(
            ledger
                .normal_side_by_code(AccountCode::PrepaidRenter)
                .await
                .unwrap(),
            usd(7, 0),
            "[{}] consumption draws down the prepaid liability",
            td.name
        );
        assert_eq!(
            ledger.account_balance(&revenue).await.unwrap().normal,
            usd(3, 0),
            "[{}] consumed prepaid becomes EARNED revenue (never revenue at deposit: rule 7)",
            td.name
        );

        // --- 3. depositor contributes $100 of key capacity ---------------
        // No cash moves: rule 7 headroom must NOT change (D38: capacity
        // claims are outside the custodial-cash equation).
        let headroom_before = ledger.solvency_report().await.unwrap().headroom;
        ledger
            .post(
                PostingRequest::new(TemplateCode::DepositCapacity, usd(100, 0))
                    .depositor("depositor-1"),
            )
            .await
            .expect("deposit.capacity");
        assert_eq!(
            ledger.solvency_report().await.unwrap().headroom,
            headroom_before,
            "[{}] capacity contributions must not touch rule-7 headroom",
            td.name
        );
        assert_eq!(
            ledger
                .normal_side_by_code(AccountCode::ClaimsContributed)
                .await
                .unwrap(),
            usd(100, 0),
            "[{}] contributed principal is a claim, capacity-backed",
            td.name
        );

        // --- 4. settlement: 8% fee (rule-8 target), rest distributable ----
        ledger
            .post(PostingRequest::new(
                TemplateCode::SettleFee,
                usd(0, 240_000),
            ))
            .await
            .expect("settle.fee");
        ledger
            .post(
                PostingRequest::new(TemplateCode::SettleDistribute, usd(2, 760_000))
                    .depositor("depositor-1"),
            )
            .await
            .expect("settle.distribute");
        assert_eq!(
            ledger.account_balance(&equity).await.unwrap().normal,
            usd(0, 240_000),
            "[{}] the fee is the operator's own claim (equity, not payable)",
            td.name
        );
        assert_eq!(
            ledger
                .normal_side_by_code(AccountCode::PayableDepositor)
                .await
                .unwrap(),
            usd(2, 760_000),
            "[{}] distributable becomes a cash-owed payable",
            td.name
        );
        assert_eq!(
            ledger.account_balance(&revenue).await.unwrap().normal,
            usd(0, 0),
            "[{}] settlement closes the revenue account to zero",
            td.name
        );

        // --- 5. rule 7 holds with the exact numbers ------------------------
        let report = ledger.solvency_check().await.expect("solvency holds");
        assert_eq!(report.escrow, usd(10, 0));
        assert_eq!(report.payable, usd(2, 760_000));
        assert_eq!(report.prepaid, usd(7, 0));
        assert_eq!(report.headroom, usd(0, 240_000));

        // --- 6. depositor withdraws $2 of settled earnings -----------------
        ledger
            .post(
                PostingRequest::new(TemplateCode::WithdrawPayable, usd(2, 0))
                    .depositor("depositor-1"),
            )
            .await
            .expect("withdraw.payable");
        let report = ledger.solvency_check().await.expect("solvency holds");
        assert_eq!(
            report.escrow,
            usd(8, 0),
            "[{}] cash left with the depositor",
            td.name
        );
        assert_eq!(report.payable, usd(0, 760_000));
        assert_eq!(
            report.headroom,
            usd(0, 240_000),
            "[{}] headroom unchanged by withdrawal",
            td.name
        );

        // --- 7. overdraw is refused, atomically ---------------------------
        let err = ledger
            .post(PostingRequest::new(TemplateCode::PrepaidConsume, usd(8, 0)).renter("renter-1"))
            .await
            .expect_err("only $7 prepaid — must refuse");
        assert!(
            err.to_string().contains("insufficient"),
            "[{}] wrong refusal reason: {err}",
            td.name
        );
        // Nothing leaked from the refused posting:
        assert_eq!(
            ledger
                .normal_side_by_code(AccountCode::PrepaidRenter)
                .await
                .unwrap(),
            usd(7, 0),
            "[{}] refused posting must leave no trace",
            td.name
        );
        ledger
            .reconcile()
            .await
            .expect("[journey] cache matches truth");
        assert!(
            ledger.trial_balance().await.unwrap().balanced(),
            "[{}] the books balance",
            td.name
        );
    }
}
