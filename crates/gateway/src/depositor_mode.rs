//! Depositor-mode routing — Session A's STUB INTERFACE (the A/B contract).
//!
//! Doctrine-suite mode (AGENTS.md step 14): every body below is `todo!()`.
//! The interface itself is the pre-agreed contract (workflow step 11);
//! Session B implements these pieces and wires them into the chat path —
//! and NEVER edits a test. The specification is the suite:
//! `tests/m9_doctrine.rs` (journeys, public API only) plus the ledger's
//! `settle` module and its suite (the D43 split).
//!
//! WHAT THIS MODULE OWNS (SPEC M9 + D43):
//! * the BUCKET REGISTRY — resolving a requested model to its bucket
//!   (provider + model family), the accounting dimension;
//! * the PER-DEPOSITOR KEY REGISTRY — a bucket's eligible keys with each
//!   depositor's contributed/burned micros in that bucket;
//! * the ROUTING POLICY trait — pro-rata BY CLAIMS.CONTRIBUTED (D43:
//!   pro-rata applies to ROUTING WEIGHTS, never to earnings attribution);
//! * BURN ATTRIBUTION — batching settled usage rows into attributed burns
//!   (attaches at SETTLE, never at reserve — A-020, invariant 11);
//! * the PER-BUCKET OVERDRAW GUARD — no depositor's key is used beyond
//!   their contributed value in that bucket (invariant 6).
//!
//! NAMING (M9-A GO refinement 2026-11-02 — ONE word everywhere): the
//! contributed-value source is `claims.contributed` (per-depositor ledger
//! account); per-bucket contributed amounts are the positions in
//! migration 0016 that must sum back to it. There is NO weight column in
//! the schema by design — the weight IS the claims.contributed-derived
//! per-bucket contributed amount, so no second truth can drift.
//!
//! SELECTION ORDER on the hot path (the contract B wires): quota
//! reserve (member-scoped, A-020) → bucket resolution → eligibility
//! (bucket fit + healthy + not revoked, all BEFORE any upstream call) →
//! weighted pick via [`RoutingPolicy`] → bucket-capacity reservation →
//! upstream call → settle (usage row + attribution) or release.

// STUB MODULE: every body below is todo!() and every parameter exists
// only for the A/B contract — they are deliberately unused until the
// bodies are real. Session B REMOVES this allow as it implements (it
// must not outlive the stubs).
#![allow(unused_variables)]

use tributary_common::db::Db;
use tributary_ledger::money::MicroUsd;
use tributary_ledger::settle::Bucket;

/// Re-exported so the data plane speaks the SAME bucket type as the
/// books (defined once in `tributary_ledger::settle` — buckets are an
/// accounting concept per D43; two definitions could drift apart).
pub use tributary_ledger::settle::Bucket as AccountingBucket;

/// One depositor-contributed key, as the routing layer sees it: WHICH
/// bucket it serves, WHOSE contribution it carries, and that depositor's
/// position in the bucket. This is the registry row — not the provider
/// key itself (ciphertext never comes near routing decisions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositorKey {
    /// `provider_keys.id` — identifies the row for selection/health.
    pub provider_key_id: String,
    /// The depositor whose contribution this key is (attribution target).
    pub depositor_id: String,
    /// The bucket this key serves (its provider + declared model family).
    pub bucket: Bucket,
    /// The depositor's contributed micros IN THIS BUCKET (from
    /// `depositor_bucket_positions` — sums back to claims.contributed).
    pub contributed_micros: MicroUsd,
    /// The depositor's already-metered burn in this bucket (exhaustion
    /// accounting against `contributed_micros`).
    pub burned_micros: MicroUsd,
    /// False ⇒ the key is skipped BEFORE any upstream call (health
    /// 'invalid' or status 'revoked' — invariant 5).
    pub healthy: bool,
}

/// How a pool picks a key among the eligible ones. A trait (not a
/// function) so the policy is a SEAM (SPEC §15: growth via trait seams)
/// — `ProRataByClaims` is the M9 policy; a future pool could plug in
/// others without core surgery.
pub trait RoutingPolicy: Send + Sync {
    /// The routing weight of an eligible key — D43: pro-rata by
    /// `claims.contributed` (per-bucket contributed micros). The suite
    /// pins `weight == contributed_micros` for `ProRataByClaims`.
    fn weight(&self, key: &DepositorKey) -> MicroUsd;

    /// Picks one key from the ELIGIBLE slice given a deterministic draw
    /// (0..Σweights). Taking the draw as a parameter keeps selection
    /// deterministic and testable — invariant 8's tolerance test and any
    /// future replay/reconciliation depend on that.
    fn select<'k>(&self, eligible: &'k [DepositorKey], draw: u64) -> Option<&'k DepositorKey>;
}

/// The M9 routing policy: weights proportional to claims.contributed.
/// This is where D43's "pro-rata applies to ROUTING WEIGHTS" lives —
/// NEVER to earnings attribution (that is `settle::split_settlement`).
pub struct ProRataByClaims;

impl RoutingPolicy for ProRataByClaims {
    /// Weight = the depositor's contributed micros in the key's bucket.
    fn weight(&self, key: &DepositorKey) -> MicroUsd {
        todo!("M9 Session B: weight is claims.contributed-derived contributed_micros (D43)")
    }

    /// Weighted pick over eligible keys; `draw` in 0..Σweights.
    fn select<'k>(&self, eligible: &'k [DepositorKey], draw: u64) -> Option<&'k DepositorKey> {
        todo!("M9 Session B: deterministic weighted selection over eligible keys")
    }
}

/// Filters a bucket's keys down to those that MAY serve: the bucket
/// matches AND the key is healthy AND the depositor has unexhausted
/// capacity. Invariant 5: this runs BEFORE any upstream call — a key
/// that cannot serve is never dialed.
pub fn eligible_keys<'k>(keys: &'k [DepositorKey], bucket: &Bucket) -> Vec<&'k DepositorKey> {
    todo!("M9 Session B: bucket-fit + health + remaining-capacity filter")
}

/// Resolves a requested model id to the bucket that serves it in this
/// pool (provider + model family) — the registry read that anchors all
/// bucket accounting. `None` ⇒ no depositor key serves this model here.
pub async fn bucket_for_model(db: &Db, pool_id: &str, model: &str) -> sqlx::Result<Option<Bucket>> {
    todo!("M9 Session B: model → bucket resolution (the bucket registry)")
}

/// Loads one bucket's depositor keys for routing (the per-depositor key
/// registry view over provider_keys × depositor_bucket_positions).
pub async fn keys_in_bucket(
    db: &Db,
    pool_id: &str,
    bucket: &Bucket,
) -> sqlx::Result<Vec<DepositorKey>> {
    todo!("M9 Session B: registry read — a bucket's keys with positions")
}

/// One attributed burn: a settled usage row naming the depositor whose
/// key burned, the bucket, and the metered cost. Produced by batching
/// usage rows — invariant 1: the batch must reconcile to per-key burn
/// totals EXACTLY (every row attributed to exactly one depositor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurnAttribution {
    pub usage_event_id: String,
    pub depositor_id: String,
    pub bucket: Bucket,
    pub burn_micros: MicroUsd,
}

/// Batches a period's settled usage rows (pool, from/to epoch-ms) into
/// attributed burns — the settle-time hand-off to
/// `tributary_ledger::settle::split_settlement`. Unattributed rows in a
/// depositor-mode pool are a doctrine violation the suite hunts.
pub async fn attribute_period_burns(
    db: &Db,
    pool_id: &str,
    from_ms: i64,
    to_ms: i64,
) -> sqlx::Result<Vec<BurnAttribution>> {
    todo!("M9 Session B: batch usage rows into attributed burns (settle, never reserve)")
}

/// The overdraw guard's refusal (invariant 6): the estimated burn would
/// push a depositor's key past their contributed value in the bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverdrawRefused;

/// Formats the refusal for logs/API (no secret material anywhere near).
impl std::fmt::Display for OverdrawRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "bucket capacity exhausted: burn would exceed the depositor's contributed value in this bucket"
        )
    }
}

/// `std::error::Error` so the guard composes with `?` in the chat path.
impl std::error::Error for OverdrawRefused {}

/// Reserves estimated burn against the depositor's remaining capacity in
/// the bucket, BEFORE the upstream call (the A-020 reserve step, bucket
/// edition). Refusal ⇒ the request never dials upstream.
pub async fn reserve_bucket_capacity(
    db: &Db,
    provider_key_id: &str,
    estimate: MicroUsd,
) -> Result<(), OverdrawRefused> {
    todo!("M9 Session B: atomic per-bucket capacity reservation")
}

/// Settles a reservation at response end with the ACTUAL metered burn
/// (A-020 settle: estimate → truth). On error/cancel paths the caller
/// releases instead (capacity returns, nothing attributes).
pub async fn settle_bucket_capacity(
    db: &Db,
    provider_key_id: &str,
    estimate: MicroUsd,
    actual: MicroUsd,
) -> sqlx::Result<()> {
    todo!("M9 Session B: reservation settle (actual burn) — attribution attaches HERE")
}

/// Releases a reservation on error/cancel (A-020 release): the estimate
/// returns to the bucket, no attribution is written.
pub async fn release_bucket_capacity(
    db: &Db,
    provider_key_id: &str,
    estimate: MicroUsd,
) -> sqlx::Result<()> {
    todo!("M9 Session B: reservation release on error/cancel")
}
