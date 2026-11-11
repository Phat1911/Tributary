//! M9 DOCTRINE SUITE — Session A (doctrine-suite mode, AGENTS.md step 14).
//!
//! Every test in this file is one DOCTRINE SENTENCE from the M9 kickoff,
//! named exactly as itself. The suite is the SPECIFICATION: it is
//! EXPECTED-RED on `todo!()` stubs until Session B implements
//! `tributary_ledger::settle`. Session B never edits these tests — a test
//! that must change is a doctrine question and routes back through the
//! user (rule 6, extended to doctrine).
//!
//! The doctrine under test (SPEC D43, founder rulings 2026-11-01):
//!   * Pro-rata applies to ROUTING WEIGHTS, never to earnings
//!     attribution — EARNINGS FOLLOW THE BURN.
//!   * A depositor earns nothing in a period where their credits burned
//!     nothing.
//!   * Buckets (provider + model family) are the accounting dimension:
//!     earnings never cross buckets.
//!   * Σ fee + Σ payouts + undistributed == Σ distributable, to the
//!     micro-unit, deterministically.
//!
//! Money is integer micro-USD everywhere; the proportionality bounds
//! below use i128 cross-multiplication, never floats (a float used for
//! money is a security finding).
//!
//! Residents (test name = doctrine sentence → kickoff invariant #):
//!   1. every_consumption_…reconcile_to_per_key_burn_totals_exactly (#1)
//!   2. a_depositor_earns_nothing_…burned_nothing (#2)
//!   3. a_requests_burn_…never_cross_buckets (#3)
//!   4. pro_rata_applies_to_routing_weights…follow_the_burn (#4)
//!   5. sigma_posted_payables_…deterministic_remainder_handling (#7)

mod harness;

use harness::test_dbs;
use tributary_common::db::Db;
use tributary_ledger::chart::{AccountCode, OwnerKind, PLATFORM_OWNER_ID};
use tributary_ledger::money::MicroUsd;
use tributary_ledger::settle::{Bucket, BurnRecord, Payout, split_settlement};
use tributary_ledger::{Ledger, PostingRequest, SqlLedger, TemplateCode};

// ---------------------------------------------------------------------------
// Harness — deterministic pseudo-randomness, no new deps (M7 properties
// precedent: seeded LCG walks keep every run reproducible in CI logs).
// ---------------------------------------------------------------------------

/// A tiny linear congruential generator: deterministic, seedable, and
/// dependency-free. NOT cryptographic — it only shapes test inputs.
struct Lcg(u64);

impl Lcg {
    /// New walk from an explicit seed (each seed is a distinct scenario).
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Advances the state; Knuth's 64-bit constants give a full period.
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// Uniform-ish value below `n` (0 when n == 0).
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

/// A bucket value for tests.
fn bucket(provider: &str, family: &str) -> Bucket {
    Bucket {
        provider: provider.to_string(),
        model_family: family.to_string(),
    }
}

/// One attributed burn record for tests.
fn burn(depositor: &str, bucket: &Bucket, micros: i64) -> BurnRecord {
    BurnRecord {
        depositor_id: depositor.to_string(),
        bucket: bucket.clone(),
        burn_micros: MicroUsd::new(micros),
    }
}

/// Dollars → MicroUsd for readable amounts (integer math only).
const fn usd(whole: i64, micros: i64) -> MicroUsd {
    MicroUsd::new(whole * 1_000_000 + micros)
}

/// A depositor's total across all payouts in a split.
fn total_for(split: &tributary_ledger::settle::SettlementSplit, depositor: &str) -> MicroUsd {
    let mut total: i64 = 0;
    for payout in &split.payouts {
        if payout.depositor_id == depositor {
            // Checked add via MicroUsd to keep overflow loud even in tests.
            total = MicroUsd::new(total)
                .checked_add(payout.amount)
                .expect("payout sum overflow")
                .micros();
        }
    }
    MicroUsd::new(total)
}

/// Σ payouts (i64 is plenty: tests stay far from i64::MAX on purpose).
fn sum_payouts(split: &tributary_ledger::settle::SettlementSplit) -> i64 {
    split.payouts.iter().map(|p| p.amount.micros()).sum()
}

/// Asserts `payout` is within ONE integer-rounding step of the ideal
/// share `dist * burn_d / burn_total`, via i128 cross-multiplication —
/// exact proportionality is impossible with integers; ONE micro-unit of
/// remainder slack per depositor per bucket is the deterministic floor.
fn assert_proportional(payout: i64, dist: i64, burn_d: i64, burn_total: i64, ctx: &str) {
    assert!(burn_total > 0, "{ctx}: burn_total must be positive");
    let lhs = (payout as i128) * (burn_total as i128);
    let rhs = (dist as i128) * (burn_d as i128);
    let slack = burn_total as i128; // one rounding step of the denominator
    assert!(
        (lhs - rhs).abs() <= slack,
        "{ctx}: payout {payout} not proportional (dist {dist}, burn {burn_d}/{burn_total})"
    );
}

// ---------------------------------------------------------------------------
// Invariant 1 — attribution is exact and postings reconcile to burn totals
// ---------------------------------------------------------------------------

/// #1: Every consumption is attributed to exactly the depositor whose key
/// burned; postings batch over usage rows and reconcile to per-key burn
/// totals exactly.
///
/// Property walk: seeded burn lists across depositors and buckets, then
/// (a) the split attributes every payout to a depositor who actually
/// burned in that bucket (zero burn in a bucket ⇒ zero payout there),
/// (b) each burning bucket distributes exactly its distributable, each
/// depositor exactly proportional (≤ 1 micro rounding), (c) the split is
/// deterministic, and (d) POSTING the batch through the PUBLIC trait
/// reconciles: every depositor's payable equals their burn-derived total
/// and the books still balance.
#[tokio::test]
async fn every_consumption_is_attributed_to_exactly_the_depositor_whose_key_burned_and_postings_batch_over_usage_rows_and_reconcile_to_per_key_burn_totals_exactly()
 {
    for seed in [3u64, 17, 41] {
        let mut rng = Lcg::new(seed);

        // Scenario: 2–4 depositors, each staked in a nonempty subset of
        // 3 buckets (stakes exist so "burned nothing" depositors appear
        // naturally), then 12–30 burn rows ONLY along staked membership
        // (a depositor's keys can only burn inside buckets they staked).
        let depositor_count = 2 + rng.below(3) as usize; // 2..=4
        let depositors: Vec<String> = (0..depositor_count).map(|i| format!("dep-{i}")).collect();
        let buckets = [
            bucket("openai", "fam-a"),
            bucket("openai", "fam-b"),
            bucket("gemini", "fam-c"),
        ];
        let staked: Vec<Vec<bool>> = depositors
            .iter()
            .map(|_| {
                (0..buckets.len())
                    .map(|b| rng.below(2) == 0 || b == 0)
                    .collect()
            })
            .collect(); // bucket 0 always has at least the last-forced staker… made certain below
        // Guarantee bucket 0 has a staked depositor (forces ≥1 burning bucket).
        let staked = {
            let mut s = staked;
            s[0][0] = true;
            s
        };

        let mut burns: Vec<BurnRecord> = Vec::new();
        for _ in 0..(12 + rng.below(19)) {
            // Pick a bucket, then a depositor staked there (retry until
            // found — staked membership makes some buckets empty by design).
            loop {
                let b = rng.below(buckets.len() as u64) as usize;
                let candidates: Vec<usize> =
                    (0..depositors.len()).filter(|&d| staked[d][b]).collect();
                if let Some(&d) = candidates.get(rng.below(candidates.len() as u64) as usize) {
                    burns.push(burn(
                        &depositors[d],
                        &buckets[b],
                        1 + rng.below(50_000) as i64,
                    ));
                    break;
                }
            }
        }
        // Distributable per bucket: random positive amounts.
        let distributable: Vec<(Bucket, MicroUsd)> = buckets
            .iter()
            .map(|b| (b.clone(), MicroUsd::new(1 + rng.below(10_000_000) as i64)))
            .collect();

        // fee 0 for this invariant — fee arithmetic is invariant #7's job.
        let split = split_settlement(&distributable, &burns, 0);
        assert_eq!(
            split.fee,
            MicroUsd::new(0),
            "seed {seed}: fee must be 0 at 0 bps"
        );

        // Which buckets actually burned, with their burn totals.
        let mut burn_totals = [0i64, 0, 0];
        for record in &burns {
            let idx = buckets
                .iter()
                .position(|b| b == &record.bucket)
                .expect("burn bucket is one of the three");
            burn_totals[idx] += record.burn_micros.micros();
        }

        for (idx, b) in buckets.iter().enumerate() {
            let dist_b = distributable[idx].1.micros();
            let in_bucket: Vec<&Payout> = split.payouts.iter().filter(|p| p.bucket == *b).collect();
            if burn_totals[idx] == 0 {
                // Burnless bucket: nobody earns (D43), distributable is
                // returned, never crossed — pinned fully in test #3; here
                // only the "no payout" half.
                assert!(
                    in_bucket.is_empty(),
                    "seed {seed}: burnless bucket {b:?} must pay nobody"
                );
                continue;
            }
            // (b) the bucket distributes EXACTLY its distributable…
            let paid: i64 = in_bucket.iter().map(|p| p.amount.micros()).sum();
            assert_eq!(
                paid, dist_b,
                "seed {seed}: bucket {b:?} must pay out exactly its distributable"
            );
            // …(a) every payout goes to a depositor who burned there,
            // exactly proportionally; zero burn ⇒ zero payout.
            for d in &depositors {
                let burn_d: i64 = burns
                    .iter()
                    .filter(|r| &r.depositor_id == d && r.bucket == *b)
                    .map(|r| r.burn_micros.micros())
                    .sum();
                let payout_d: i64 = in_bucket
                    .iter()
                    .filter(|p| &p.depositor_id == d)
                    .map(|p| p.amount.micros())
                    .sum();
                if burn_d == 0 {
                    assert_eq!(
                        payout_d, 0,
                        "seed {seed}: {d} burned nothing in {b:?} — must earn nothing there"
                    );
                } else {
                    assert_proportional(
                        payout_d,
                        dist_b,
                        burn_d,
                        burn_totals[idx],
                        &format!("seed {seed} {d} in {b:?}"),
                    );
                }
            }
        }

        // (c) determinism — the same inputs must produce the same split
        // (M11's Merkle root is a function of the settlement).
        let again = split_settlement(&distributable, &burns, 0);
        assert_eq!(split, again, "seed {seed}: split must be deterministic");

        // (d) the posting half, through the PUBLIC trait only: fund
        // revenue with exactly the burning buckets' distributable (the
        // renter prepaid story), post every payout as settle.distribute,
        // and reconcile per-depositor payables to the burn-derived totals.
        let total_dist: i64 = burn_totals
            .iter()
            .enumerate()
            .map(|(i, _)| distributable[i].1.micros())
            .sum();
        for td in test_dbs().await {
            let td_name = td.name; // format strings capture plain identifiers only
            let db: &Db = &td.pool;
            let ledger = SqlLedger::new(db.clone());
            for code in [
                AccountCode::CashEscrow,
                AccountCode::RevenueUsage,
                AccountCode::PrepaidRenter,
            ] {
                let owner = match code {
                    AccountCode::PrepaidRenter => (OwnerKind::Renter, "renter-1"),
                    _ => (OwnerKind::Operator, PLATFORM_OWNER_ID),
                };
                ledger
                    .open_account(code, owner.0, owner.1)
                    .await
                    .expect("open platform/renter");
            }
            for d in &depositors {
                ledger
                    .open_account(AccountCode::PayableDepositor, OwnerKind::Depositor, d)
                    .await
                    .expect("open depositor payable");
            }
            ledger
                .post(
                    PostingRequest::new(TemplateCode::PrepaidFund, MicroUsd::new(total_dist))
                        .renter("renter-1"),
                )
                .await
                .expect("prepaid.fund");
            ledger
                .post(
                    PostingRequest::new(TemplateCode::PrepaidConsume, MicroUsd::new(total_dist))
                        .renter("renter-1"),
                )
                .await
                .expect("prepaid.consume");

            // THE BATCH: one settle.distribute per payout, through the
            // trait — postings batch over the burn records exactly.
            for payout in &split.payouts {
                ledger
                    .post(
                        PostingRequest::new(TemplateCode::SettleDistribute, payout.amount)
                            .depositor(&payout.depositor_id)
                            .reference(&format!("seed-{seed}")),
                    )
                    .await
                    .expect("settle.distribute");
            }
            for d in &depositors {
                assert_eq!(
                    payable_of(&ledger, d).await,
                    total_for(&split, d),
                    "[{td_name}] seed {seed}: {d}'s posted payable must equal the burn-derived total",
                );
            }
            assert!(
                ledger
                    .trial_balance()
                    .await
                    .expect("trial balance")
                    .balanced(),
                "[{td_name}] seed {seed}: books balance after the batch"
            );
            ledger.reconcile().await.expect("cache matches entries");
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant 2 — zero burn ⇒ zero earnings
// ---------------------------------------------------------------------------

/// #2: A depositor earns nothing in a period where their credits burned
/// nothing.
///
/// Seeded walks where one depositor ("silent") holds stakes in every
/// bucket (EQUAL to the loudest burner's) but never burns; the split
/// must pay them exactly zero — regardless of fees, bucket counts, and
/// other depositors' burns. This is the anti-blending pin: share-based
/// (pro-rata-by-claims) earnings would pay "silent" and fail here.
#[test]
fn a_depositor_earns_nothing_in_a_period_where_their_credits_burned_nothing() {
    for seed in [7u64, 11, 29] {
        let mut rng = Lcg::new(seed);
        let buckets = [bucket("openai", "fam-a"), bucket("gemini", "fam-b")];
        let mut burns = Vec::new();
        for b in &buckets {
            // One "loud" depositor burns; "silent" never does.
            let rows = 1 + rng.below(6);
            for _ in 0..rows {
                burns.push(burn("loud", b, 1 + rng.below(99_999) as i64));
            }
        }
        let distributable: Vec<(Bucket, MicroUsd)> = buckets
            .iter()
            .map(|b| (b.clone(), MicroUsd::new(1 + rng.below(9_999_999) as i64)))
            .collect();
        let fee_bps = 500 + rng.below(501) as i64; // inside the rule-8 band
        let split = split_settlement(&distributable, &burns, fee_bps);

        assert_eq!(
            total_for(&split, "silent"),
            MicroUsd::new(0),
            "seed {seed}: silent staked everywhere but burned nothing — must earn nothing",
        );
        // And the period still conserves (fee + payouts + undistributed).
        let total_dist: i64 = distributable.iter().map(|(_, m)| m.micros()).sum();
        assert_eq!(
            split.fee.micros() + sum_payouts(&split) + split.undistributed.micros(),
            total_dist,
            "seed {seed}: conservation must hold even with silent stakes present",
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant 3 — earnings never cross buckets
// ---------------------------------------------------------------------------

/// #3: A request's burn is attributed within the bucket that serves it;
/// earnings never cross buckets.
///
/// dep-a staked in buckets X and Y; dep-b staked ONLY in Y. All burns
/// happen in X (by a). X's distributable belongs to a alone; Y's
/// distributable goes to NOBODY (returned undistributed) — not to b (who
/// staked there but burned nothing) and not to a (who burned, but in the
/// other bucket). The fee is charged on burned buckets only.
#[test]
fn a_requests_burn_is_attributed_within_the_bucket_that_serves_it_and_earnings_never_cross_buckets()
{
    let x = bucket("openai", "fam-x");
    let y = bucket("openai", "fam-y");
    let burns = vec![burn("dep-a", &x, 500_000), burn("dep-a", &x, 300_000)];
    let distributable = vec![(x.clone(), usd(9, 0)), (y.clone(), usd(4, 0))];

    let split = split_settlement(&distributable, &burns, 800);

    // Every payout sits in the bucket that burned…
    for payout in &split.payouts {
        assert_eq!(
            payout.bucket, x,
            "payouts may only come from the burning bucket"
        );
    }
    // …a earned ALL of X's after-fee distributable (a burned all of X)…
    assert_eq!(
        split.fee,
        MicroUsd::new(720_000),
        "fee is 8% of the burned bucket only"
    );
    assert_eq!(
        total_for(&split, "dep-a"),
        usd(8, 280_000),
        "X after-fee belongs to its sole burner"
    );
    // …b — staked in Y, silent in X — earned nothing…
    assert_eq!(
        total_for(&split, "dep-b"),
        MicroUsd::new(0),
        "b never burned: nothing, anywhere"
    );
    // …and Y's distributable was RETURNED, never crossed over.
    assert_eq!(
        split.undistributed,
        usd(4, 0),
        "a burnless bucket's distributable is returned, not reallocated"
    );
}

// ---------------------------------------------------------------------------
// Invariant 4 — the D43 headline
// ---------------------------------------------------------------------------

/// #4: Pro-rata applies to routing weights, never to earnings attribution
/// — earnings follow the burn.
///
/// THE sentence. Two depositors with EQUAL claims.contributed (100 USD
/// each) but UNEQUAL burns (9:1). Share-blended earnings would pay 50/50;
/// D43 pays 9:1 — earnings follow the burn. (The routing half of the
/// sentence — equal claims DO mean equal routing weights — is pinned in
/// the gateway suite, invariant #8, against `ProRataByClaims::weight`.)
#[test]
fn pro_rata_applies_to_routing_weights_never_to_earnings_attribution_earnings_follow_the_burn() {
    let b = bucket("openai", "gpt-4o");
    let burns = vec![burn("dep-a", &b, 900_000), burn("dep-b", &b, 100_000)];
    let distributable = vec![(b, usd(10, 0))];

    let split = split_settlement(&distributable, &burns, 800);

    // Exact numbers, chosen to divide cleanly: fee 8% of 10 USD = 0.80;
    // after-fee 9.20 USD splits 9:1 into 8.28 / 0.92 — NO remainder
    // ambiguity can hide a blend.
    assert_eq!(split.fee, MicroUsd::new(800_000));
    assert_eq!(
        total_for(&split, "dep-a"),
        MicroUsd::new(8_280_000),
        "equal claims, 9× burn ⇒ 9× earnings"
    );
    assert_eq!(
        total_for(&split, "dep-b"),
        MicroUsd::new(920_000),
        "equal claims, 1× burn ⇒ 1× earnings"
    );
    assert_eq!(split.undistributed, MicroUsd::new(0));
    // And had earnings been blended by claims (50/50 of 9.20), dep-a
    // would hold 4.60 — the exact wrong number, stated so the failure
    // message teaches:
    assert_ne!(
        total_for(&split, "dep-a"),
        MicroUsd::new(4_600_000),
        "earnings were share-blended — D43 violation",
    );
    // Deterministic, as ever.
    let again = split_settlement(&distributable, &burns, 800);
    assert_eq!(split, again);
}

// ---------------------------------------------------------------------------
// Invariant 7 — conservation to the micro-unit, deterministic remainder
// ---------------------------------------------------------------------------

/// #7: Σ posted payables equals total distributable to the micro-unit,
/// with deterministic remainder handling.
///
/// Deliberately awkward divisions (1_000_003 across three equal burners,
/// 999_999 across a 2:1 pair): no micro-unit may vanish, every payout is
/// within ONE rounding step of the ideal share, the same inputs always
/// produce the identical split, and POSTING the batch through the public
/// trait leaves Σ payable == total distributable exactly with the books
/// balanced.
#[tokio::test]
async fn sigma_posted_payables_equals_total_distributable_to_the_micro_unit_with_deterministic_remainder_handling()
 {
    let x = bucket("openai", "fam-x");
    let y = bucket("openai", "fam-y");
    let burns = vec![
        burn("dep-a", &x, 1),
        burn("dep-b", &x, 1),
        burn("dep-c", &x, 1),
        burn("dep-a", &y, 2),
        burn("dep-b", &y, 1),
    ];
    let dist_x = 1_000_003i64;
    let dist_y = 999_999i64;
    let distributable = vec![
        (x.clone(), MicroUsd::new(dist_x)),
        (y.clone(), MicroUsd::new(dist_y)),
    ];
    let total_dist = dist_x + dist_y;

    let split = split_settlement(&distributable, &burns, 0);

    // No micro-unit vanishes…
    assert_eq!(
        split.fee.micros() + sum_payouts(&split) + split.undistributed.micros(),
        total_dist,
        "fee + payouts + undistributed must equal Σ distributable exactly",
    );
    // …each payout is within one rounding step of its ideal share…
    assert_proportional(payout_of(&split, "dep-a", &x), dist_x, 1, 3, "dep-a/x");
    assert_proportional(payout_of(&split, "dep-b", &x), dist_x, 1, 3, "dep-b/x");
    assert_proportional(payout_of(&split, "dep-c", &x), dist_x, 1, 3, "dep-c/x");
    assert_proportional(payout_of(&split, "dep-a", &y), dist_y, 2, 3, "dep-a/y");
    assert_proportional(payout_of(&split, "dep-b", &y), dist_y, 1, 3, "dep-b/y");
    // …and the remainder assignment is DETERMINISTIC.
    let again = split_settlement(&distributable, &burns, 0);
    assert_eq!(
        split, again,
        "identical inputs must yield the identical split"
    );

    // The posting half: fund revenue with exactly the distributable,
    // post every payout, and Σ payable must equal total distributable
    // to the micro-unit.
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let db: &Db = &td.pool;
        let ledger = SqlLedger::new(db.clone());
        for code in [
            AccountCode::CashEscrow,
            AccountCode::RevenueUsage,
            AccountCode::PrepaidRenter,
        ] {
            let owner = match code {
                AccountCode::PrepaidRenter => (OwnerKind::Renter, "renter-7"),
                _ => (OwnerKind::Operator, PLATFORM_OWNER_ID),
            };
            ledger
                .open_account(code, owner.0, owner.1)
                .await
                .expect("open platform/renter");
        }
        for d in ["dep-a", "dep-b", "dep-c"] {
            ledger
                .open_account(AccountCode::PayableDepositor, OwnerKind::Depositor, d)
                .await
                .expect("open payable");
        }
        ledger
            .post(
                PostingRequest::new(TemplateCode::PrepaidFund, MicroUsd::new(total_dist))
                    .renter("renter-7"),
            )
            .await
            .expect("prepaid.fund");
        ledger
            .post(
                PostingRequest::new(TemplateCode::PrepaidConsume, MicroUsd::new(total_dist))
                    .renter("renter-7"),
            )
            .await
            .expect("prepaid.consume");
        for payout in &split.payouts {
            ledger
                .post(
                    PostingRequest::new(TemplateCode::SettleDistribute, payout.amount)
                        .depositor(&payout.depositor_id),
                )
                .await
                .expect("settle.distribute");
        }
        let mut posted_payables: i64 = 0;
        for d in ["dep-a", "dep-b", "dep-c"] {
            posted_payables += payable_of(&ledger, d).await.micros();
        }
        assert_eq!(
            posted_payables, total_dist,
            "[{td_name}] Σ posted payables must equal total distributable to the micro-unit",
        );
        assert!(
            ledger
                .trial_balance()
                .await
                .expect("trial balance")
                .balanced(),
            "[{td_name}] books balance after settlement",
        );
    }
}

/// A single (depositor, bucket) payout amount from a split (0 if absent).
fn payout_of(
    split: &tributary_ledger::settle::SettlementSplit,
    depositor: &str,
    bucket: &Bucket,
) -> i64 {
    split
        .payouts
        .iter()
        .filter(|p| p.depositor_id == depositor && p.bucket == *bucket)
        .map(|p| p.amount.micros())
        .sum()
}

/// One depositor's posted payable.depositor balance, via the PUBLIC trait
/// (`account_id` + `account_balance` — the per-owner read the M7 journey
/// uses; `normal_side_by_code` aggregates across owners and cannot see
/// one depositor).
async fn payable_of(ledger: &SqlLedger, depositor: &str) -> MicroUsd {
    let id = ledger
        .account_id(
            AccountCode::PayableDepositor,
            OwnerKind::Depositor,
            depositor,
        )
        .await
        .expect("account id lookup")
        .expect("payable account was opened above");
    ledger
        .account_balance(&id)
        .await
        .expect("balance read")
        .normal
}
