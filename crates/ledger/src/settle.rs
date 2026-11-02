//! M9 settlement stub — D43's earnings doctrine, as an INTERFACE.
//!
//! This module is Session A's half of the doctrine-suite contract
//! (AGENTS.md step 14; workflow step 11 precedent): every `todo!()` body
//! is Session B's to implement. The doctrine suite
//! (`tests/m9_doctrine.rs`) is the specification — B implements to green
//! and never edits a test.
//!
//! THE DOCTRINE (D43, founder rulings 2026-11-01):
//! * Pro-rata applies to ROUTING WEIGHTS, never to earnings attribution —
//!   EARNINGS FOLLOW THE BURN. A depositor earns nothing in a period
//!   where their credits burned nothing (equal `claims.contributed`
//!   stakes do NOT mean equal earnings — unequal burns mean unequal
//!   earnings).
//! * The BUCKET (provider + model family) is the accounting dimension:
//!   a bucket's distributable is shared only by the depositors whose
//!   keys burned inside that bucket, in proportion to their burns there.
//!   Earnings never cross buckets.
//!
//! NAMING (the M9-A GO refinement, 2026-11-02 — ONE word everywhere):
//! the single contributed-value source is `claims.contributed`, the
//! per-depositor liability account of migration 0015. The per-bucket
//! amounts in `depositor_bucket_positions` (migration 0016) are POSITIONS
//! that must sum back to it. `capacity.contributed` is only the asset
//! leg of the `deposit.capacity` posting — routing and overdraw logic
//! never read it.
//!
//! POSTING TEMPLATES: M9 needs NO new template — `settle.fee` +
//! `settle.distribute` (M7) already carry the right legs; D43 changes
//! WHICH depositor gets WHICH amount, not the double-entry shape. That
//! is a constraint on B: the attribution lives in the split below, not
//! in new ledger mechanics.

use crate::money::MicroUsd;
use tributary_common::db::Db;

/// The accounting dimension (D43): provider + model family. Defined HERE,
/// in the books' crate, because buckets are an ACCOUNTING concept that
/// lives off-chain — the gateway re-exports this one type so the data
/// plane and the books can never disagree about what a bucket is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
// Debug/Clone/Eq/Hash: tests compare and dedupe buckets; Hash because a
// bucket keys the per-bucket distributable map below.
pub struct Bucket {
    /// Provider label exactly as stored on `provider_keys.provider`
    /// (e.g. "openai") — identities live in data, never code (SPEC §15).
    pub provider: String,
    /// Model family (e.g. "gpt-4o") — the family a served model resolves
    /// to; resolution is B's registry (see the gateway stub).
    pub model_family: String,
}

/// One metered burn, attributed: a usage row that has SETTLED (A-020's
/// response-end settle — attribution attaches at settle, never at
/// reserve), naming the depositor whose key burned and the bucket it
/// burned in. `burn_micros` is the row's `cost_micros`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurnRecord {
    /// The depositor whose contributed key served the request.
    pub depositor_id: String,
    /// The bucket the request burned in (where the serving key lives).
    pub bucket: Bucket,
    /// The metered cost of that usage row, integer micro-USD.
    pub burn_micros: MicroUsd,
}

/// An earned payout: depositor × bucket × amount. Buckets ride the payout
/// (not just the depositor total) because M11's Merkle leaves are
/// per-bucket positions — the on-chain registry stays the aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payout {
    pub depositor_id: String,
    pub bucket: Bucket,
    pub amount: MicroUsd,
}

/// The result of splitting a period: the operator's fee, one payout per
/// (depositor, bucket) that burned, and any distributable that could not
/// be paid out. `fee` is the SUM over buckets (the fee posts once, via
/// `settle.fee`); `undistributed` carries a burnless bucket's share —
/// D43 forbids paying it to anyone (zero burn ⇒ zero earnings), so it is
/// RETURNED, never pocketed silently: fee + Σ payouts + undistributed ==
/// Σ distributable, always, to the micro-unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementSplit {
    pub fee: MicroUsd,
    pub payouts: Vec<Payout>,
    /// Distributable of buckets with no attributed burns (0 when every
    /// listed bucket burned) — stays in revenue.usage until the operator
    /// re-releases it in a later period.
    pub undistributed: MicroUsd,
}

/// Splits each bucket's distributable between the operator fee
/// (`fee_bps` basis points, rule-8 band) and the depositors whose keys
/// burned in THAT bucket, strictly in proportion to their burns there.
///
/// Doctrine pinned by the suite:
/// * zero burn in a bucket ⇒ zero earnings from that bucket (no
///   share-blending across depositors, D43);
/// * distributable of bucket X never pays a depositor who only staked
///   in bucket Y (earnings never cross buckets);
/// * Σ payouts + fee == Σ distributable, to the micro-unit, with
///   DETERMINISTIC remainder handling (same inputs ⇒ byte-identical
///   split — M11's Merkle root must be reproducible);
/// * equal claims with unequal burns ⇒ earnings follow the burns.
pub fn split_settlement(
    distributable_by_bucket: &[(Bucket, MicroUsd)],
    burns: &[BurnRecord],
    fee_bps: i64,
) -> SettlementSplit {
    // Session B implements (doctrine suite: tests/m9_doctrine.rs). The
    // signature is the contract: pure, total, deterministic.
    // (Names stay real for the contract; the touch silences unused-param
    // warnings while the body is still a stub.)
    let _ = (distributable_by_bucket, burns, fee_bps);
    todo!("M9 Session B: implement the D43 split (earnings follow the burn, per bucket)")
}

/// Reads the per-depositor × per-bucket claim positions (migration 0016's
/// `depositor_bucket_positions`) for a pool — the M11 Merkle leaves.
///
/// The suite pins two properties through this read: every position's
/// `contributed_micros` sums (per depositor) back to that depositor's
/// `claims.contributed` book balance, and no position ever shows
/// `burned_micros > contributed_micros` (per-bucket overdraw impossible).
pub struct BucketPosition {
    pub depositor_id: String,
    pub bucket: Bucket,
    /// This bucket's share of the depositor's claims.contributed.
    pub contributed_micros: MicroUsd,
    /// Metered burn attributed to this depositor's keys in this bucket.
    pub burned_micros: MicroUsd,
    /// Settled D43 earnings credited in this bucket.
    pub earned_micros: MicroUsd,
}

/// Loads a pool's bucket positions (assertion/read path for the suite and
/// M11; the routing hot path caches, but truth is these rows).
pub async fn bucket_positions(db: &Db, pool_id: &str) -> sqlx::Result<Vec<BucketPosition>> {
    // Session B implements. Kept in the LEDGER crate because positions
    // are books-adjacent truth (D43: buckets live off-chain, in the books).
    let _ = (db, pool_id); // stub: names kept for the contract
    todo!("M9 Session B: read depositor_bucket_positions for the pool")
}
