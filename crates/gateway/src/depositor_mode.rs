//! Depositor-mode routing — the A/B contract, IMPLEMENTED (Session B, M9).
//!
//! Doctrine-suite mode (AGENTS.md step 14): the interface below was the
//! pre-agreed contract (workflow step 11); the specification was — and is —
//! the suite: `tests/m9_doctrine.rs` (journeys, public API only) plus the
//! ledger's `settle` module and its suite (the D43 split). Session B never
//! edits a test; every public signature here is byte-identical to the stub.
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
//!   their contributed value in that bucket (invariant 6);
//! * the M9 PRICING SLICE v0 — deterministic positive integer cost for
//!   metered usage on ROOT pools only (V1 keeps cost 0 until D23).
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

use std::sync::atomic::{AtomicU64, Ordering};

use sqlx::Row;
use tributary_common::db::Db;
use tributary_ledger::money::MicroUsd;
use tributary_ledger::settle::Bucket;

/// Re-exported so the data plane speaks the SAME bucket type as the
/// books (defined once in `tributary_ledger::settle` — buckets are an
/// accounting concept per D43; two definitions could drift apart).
pub use tributary_ledger::settle::Bucket as AccountingBucket;

// ---------------------------------------------------------------------------
// Pricing slice v0
// ---------------------------------------------------------------------------

/// The M9 pricing slice v0: flat 150 micro-USD per metered token — i.e.
/// $0.15 per 1K tokens. Deterministic, integer-only (money is integer
/// micro-units — a float here would be a security finding). ROOT pools
/// only: V1 org pools keep `cost_micros = 0` (pinned by the existing
/// suite, "cost stays 0 until pricing (D23)"); D23's real price table
/// owns the eventual per-model rates (documented in PLAN.md's M9 entry).
pub const PRICE_MICROS_PER_TOKEN: i64 = 150;

/// Prices one metered usage row (root pools): (prompt + completion)
/// tokens at the slice rate. Clamped inputs cannot produce negative
/// money — metering is unsigned in spirit.
pub fn price_tokens(prompt_tokens: i64, completion_tokens: i64) -> MicroUsd {
    let tokens = prompt_tokens.max(0) + completion_tokens.max(0);
    MicroUsd::new(tokens.saturating_mul(PRICE_MICROS_PER_TOKEN))
}

/// Prices the A-020 reservation estimate for a request body: the same
/// token estimator the quota engine uses (`quota::estimate_tokens`),
/// priced at the slice rate. The estimate is deliberately conservative
/// (it assumes a full completion) so the reserve step holds back at
/// least as much capacity as the actual burn will settle for.
pub fn price_estimate(body: &serde_json::Value) -> MicroUsd {
    price_tokens(crate::quota::estimate_tokens(body), 0)
}

// ---------------------------------------------------------------------------
// Draw source + policy
// ---------------------------------------------------------------------------

/// Process-wide monotonic counter feeding the routing draws: every routed
/// request consumes exactly one. Never repeats within a boot, and `mix64`
/// below scrambles it into an approximately uniform draw — no RNG needed
/// for a deterministic, well-spread sequence (M7's seeded-LCG precedent).
static DRAW_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One fresh routing draw (any u64) — the input `select` will mix.
pub fn next_draw() -> u64 {
    DRAW_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// splitmix64 (golden-gamma increment + the published finalizer): a
/// five-line integer avalanche mixer turning ANY u64 — sequential
/// counters, tiny draws, anything — into an approximately uniform u64.
/// No new dependency; same deterministic-mixer family as the M7 test LCG.
fn mix64(draw: u64) -> u64 {
    // The gamma addition is part of splitmix64 proper: without it, small
    // sequential inputs stay correlated in the high bits.
    let mut z = draw.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

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
        key.contributed_micros
    }

    /// Weighted pick over eligible keys; `draw` in 0..Σweights. ANY u64
    /// draw is valid: it is splitmix64-mixed into the weight space first,
    /// so callers may pass counters, timestamps, or test sequences —
    /// the realized distribution still matches the weights (invariant 8).
    fn select<'k>(&self, eligible: &'k [DepositorKey], draw: u64) -> Option<&'k DepositorKey> {
        // i128 sums: any realistic number of keys × i64 micros is safe.
        let total: i128 = eligible
            .iter()
            .map(|k| k.contributed_micros.micros() as i128)
            .sum();
        if total <= 0 {
            return None; // nothing eligible carries weight — nothing to pick
        }
        // Walk the cumulative-weight ladder until the mixed draw lands.
        let mut point = (mix64(draw) as i128) % total;
        eligible.iter().find(|key| {
            let weight = key.contributed_micros.micros() as i128;
            if point < weight {
                true
            } else {
                point -= weight;
                false
            }
        })
    }
}

/// The process-wide M9 policy instance (stateless — weights live on keys).
static POLICY: ProRataByClaims = ProRataByClaims;

/// The shared policy accessor for the hot path (a static because the
/// policy carries no state; `select` only reads the keys it is handed).
pub fn policy() -> &'static ProRataByClaims {
    &POLICY
}

// ---------------------------------------------------------------------------
// Bucket registry
// ---------------------------------------------------------------------------

/// True when family F serves model M (the bucket-fit rule — families are
/// data, never code): F equals M, or F is a HYPHEN-prefix of M — "gpt-4o"
/// serves "gpt-4o-mini", while "gpt" never serves "gpt4o". The hyphen
/// keeps family boundaries honest: a bare prefix would let "gpt" claim
/// "gpt-newmodel" families it never declared.
pub fn family_serves(family: &str, model: &str) -> bool {
    if family == model {
        return true;
    }
    // starts_with already guarantees family.len() is a char boundary of
    // model, so indexing the next byte for the hyphen check is safe.
    model.starts_with(family) && model.as_bytes().get(family.len()) == Some(&b'-')
}

/// Resolves a requested model id to the bucket that serves it in this
/// pool (provider + model family) — the registry read that anchors all
/// bucket accounting. `None` ⇒ no depositor key serves this model here.
///
/// Determinism: among every (key, family) that fits, the LONGEST family
/// wins (the most specific declaration); ties break by family name, then
/// provider — the same inputs always resolve to the same bucket.
pub async fn bucket_for_model(db: &Db, pool_id: &str, model: &str) -> sqlx::Result<Option<Bucket>> {
    // Every depositor key of the pool declares its families in data; the
    // scan is pool-scoped and small (a pool holds a handful of deposits).
    let sql = db.sql(
        "SELECT provider, model_families FROM provider_keys \
         WHERE pool_id = ? AND depositor_id IS NOT NULL",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(db.pool())
        .await?;
    let mut best: Option<(usize, String, String)> = None; // (len, family, provider)
    for row in &rows {
        let provider: String = row.get("provider");
        let raw_families: String = row.get("model_families");
        let Ok(families) = serde_json::from_str::<Vec<String>>(&raw_families) else {
            // Unparseable declaration (manual SQL damage): the key can
            // serve nothing it cannot name — skip it, never guess.
            continue;
        };
        for family in families {
            if !family_serves(&family, model) {
                continue;
            }
            let better = match &best {
                None => true,
                Some((len, known_family, known_provider)) => {
                    // Longer family first; then smallest family name; then
                    // smallest provider — a total, deterministic order.
                    (family.len(), family.as_str(), provider.as_str())
                        > (*len, known_family.as_str(), known_provider.as_str())
                }
            };
            if better {
                best = Some((family.len(), family.clone(), provider.clone()));
            }
        }
    }
    Ok(best.map(|(_, family, provider)| Bucket {
        provider,
        model_family: family,
    }))
}

// ---------------------------------------------------------------------------
// Key registry
// ---------------------------------------------------------------------------

/// Loads one bucket's depositor keys for routing (the per-depositor key
/// registry view over provider_keys × depositor_bucket_positions).
/// Deterministic order (provider key id) — `select` walks it stably.
pub async fn keys_in_bucket(
    db: &Db,
    pool_id: &str,
    bucket: &Bucket,
) -> sqlx::Result<Vec<DepositorKey>> {
    // LEFT JOIN: a key whose position row is missing still appears (with
    // zero contributed/burned) — eligibility then filters it out; a
    // silent drop would hide book drift from the routing layer.
    let sql = db.sql(
        "SELECT pk.id AS pk_id, pk.depositor_id, pk.health, pk.status, pk.model_families, \
           dbp.contributed_micros, dbp.burned_micros \
         FROM provider_keys pk \
         LEFT JOIN depositor_bucket_positions dbp \
           ON dbp.pool_id = pk.pool_id AND dbp.depositor_id = pk.depositor_id \
          AND dbp.provider = pk.provider AND dbp.model_family = ? \
         WHERE pk.pool_id = ? AND pk.depositor_id IS NOT NULL AND pk.provider = ? \
         ORDER BY pk.id",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&bucket.model_family)
        .bind(pool_id)
        .bind(&bucket.provider)
        .fetch_all(db.pool())
        .await?;
    let mut keys = Vec::new();
    for row in &rows {
        let raw_families: String = row.get("model_families");
        let Ok(families) = serde_json::from_str::<Vec<String>>(&raw_families) else {
            continue; // same skip rule as bucket_for_model
        };
        // The key serves THIS bucket only if it declares this family.
        if !families.iter().any(|family| family == &bucket.model_family) {
            continue;
        }
        let health: String = row.get("health");
        let status: String = row.get("status");
        keys.push(DepositorKey {
            provider_key_id: row.get("pk_id"),
            depositor_id: row.get("depositor_id"),
            bucket: bucket.clone(),
            contributed_micros: MicroUsd::new(
                row.get::<Option<i64>, _>("contributed_micros").unwrap_or(0),
            ),
            burned_micros: MicroUsd::new(row.get::<Option<i64>, _>("burned_micros").unwrap_or(0)),
            // Invariant 5: revoked ('revoked') and broken ('invalid')
            // keys are skipped BEFORE any upstream call.
            healthy: status == "active" && health != "invalid",
        });
    }
    Ok(keys)
}

/// Filters a bucket's keys down to those that MAY serve: the bucket
/// matches AND the key is healthy AND the depositor has unexhausted
/// capacity. Invariant 5: this runs BEFORE any upstream call — a key
/// that cannot serve is never dialed.
pub fn eligible_keys<'k>(keys: &'k [DepositorKey], bucket: &Bucket) -> Vec<&'k DepositorKey> {
    keys.iter()
        .filter(|key| {
            key.bucket == *bucket
                && key.healthy
                && (key.contributed_micros.micros() - key.burned_micros.micros()) > 0
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Burn attribution (settle-time batching)
// ---------------------------------------------------------------------------

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
    // Only rows that carry attribution participate (V1 rows never do);
    // ts ∈ [from, to) keeps consecutive windows non-overlapping. Ordered
    // by (ts, id) so the batch is deterministic for the same window.
    let sql = db.sql(
        "SELECT id, depositor_id, bucket_provider, bucket_model_family, cost_micros \
         FROM usage_events \
         WHERE pool_id = ? AND ts >= ? AND ts < ? AND depositor_id IS NOT NULL \
         ORDER BY ts, id",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(db.pool())
        .await?;
    Ok(rows
        .iter()
        .map(|row| BurnAttribution {
            usage_event_id: row.get("id"),
            depositor_id: row.get("depositor_id"),
            bucket: Bucket {
                provider: row.get("bucket_provider"),
                model_family: row.get("bucket_model_family"),
            },
            burn_micros: MicroUsd::new(row.get("cost_micros")),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Per-bucket overdraw guard (A-020 reserve → settle → release, bucket edition)
// ---------------------------------------------------------------------------

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

/// The (pool, depositor, provider) triple a provider key resolves to —
/// the position-row key minus the family.
struct KeyOwner {
    pool_id: String,
    depositor_id: String,
    provider: String,
}

/// Reads a provider key's ownership triple; `None` when the key does not
/// exist or is not a depositor key (callers fail closed — an unknown key
/// has no capacity to burn).
async fn key_owner(db: &Db, provider_key_id: &str) -> sqlx::Result<Option<KeyOwner>> {
    let sql = db.sql(
        "SELECT pool_id, depositor_id, provider FROM provider_keys \
         WHERE id = ? AND depositor_id IS NOT NULL",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(provider_key_id)
        .fetch_optional(db.pool())
        .await?;
    Ok(row.map(|row| KeyOwner {
        pool_id: row.get("pool_id"),
        depositor_id: row.get("depositor_id"),
        provider: row.get("provider"),
    }))
}

/// Resolves the family a capacity operation targets. With an explicit
/// bucket (the chat path always knows it): that family. Without one (the
/// single-argument public surface): the key's SOLE position — a key
/// holding several positions is ambiguous under that surface and errors
/// loudly instead of guessing which ceiling to spend from.
async fn resolve_family(
    db: &Db,
    owner: &KeyOwner,
    bucket: Option<&Bucket>,
) -> sqlx::Result<Option<String>> {
    if let Some(bucket) = bucket {
        return Ok(Some(bucket.model_family.clone()));
    }
    let sql = db.sql(
        "SELECT model_family FROM depositor_bucket_positions \
         WHERE pool_id = ? AND depositor_id = ? AND provider = ?",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&owner.pool_id)
        .bind(&owner.depositor_id)
        .bind(&owner.provider)
        .fetch_all(db.pool())
        .await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(Some(rows[0].get("model_family"))),
        _ => Err(sqlx::Error::Configuration(
            "provider key holds multiple bucket positions; use the bucket-aware form".into(),
        )),
    }
}

/// Atomically reserves estimated burn against the depositor's remaining
/// capacity in the bucket: ONE conditional UPDATE — `burned + estimate ≤
/// contributed` — is the atomicity unit on both engines (the same shape
/// as the quota engine's window reservation). `Ok(false)` = the ceiling
/// blocked the reservation (refusal).
async fn reserve_for_owner(
    db: &Db,
    owner: &KeyOwner,
    family: &str,
    estimate: MicroUsd,
) -> sqlx::Result<bool> {
    let sql = db.sql(
        "UPDATE depositor_bucket_positions \
         SET burned_micros = burned_micros + ?, updated_at = ? \
         WHERE pool_id = ? AND depositor_id = ? AND provider = ? AND model_family = ? \
           AND burned_micros + ? <= contributed_micros",
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(estimate.micros())
        .bind(crate::db::now_ms())
        .bind(&owner.pool_id)
        .bind(&owner.depositor_id)
        .bind(&owner.provider)
        .bind(family)
        .bind(estimate.micros())
        .execute(db.pool())
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Applies `delta` to the position's burned tracker with no ceiling
/// check — the settle/release half of A-020 (truth replaces estimate;
/// the ceiling was enforced at reserve time).
async fn adjust_for_owner(db: &Db, owner: &KeyOwner, family: &str, delta: i64) -> sqlx::Result<()> {
    let sql = db.sql(
        "UPDATE depositor_bucket_positions \
         SET burned_micros = burned_micros + ?, updated_at = ? \
         WHERE pool_id = ? AND depositor_id = ? AND provider = ? AND model_family = ?",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(delta)
        .bind(crate::db::now_ms())
        .bind(&owner.pool_id)
        .bind(&owner.depositor_id)
        .bind(&owner.provider)
        .bind(family)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Bucket-aware reserve — the hot path's form (the resolved bucket is
/// always known at selection time). DB failures are logged and turned
/// into refusals: the guard FAILS CLOSED (an uncertain ceiling never
/// lets a request through), matching the `OverdrawRefused`-only surface
/// the stub contract fixed for the single-argument form.
pub async fn reserve_bucket_capacity_for(
    db: &Db,
    provider_key_id: &str,
    bucket: &Bucket,
    estimate: MicroUsd,
) -> Result<(), OverdrawRefused> {
    let refused = Err(OverdrawRefused);
    let Some(owner) = key_owner(db, provider_key_id).await.map_err(|err| {
        tracing::error!("bucket capacity reserve lookup failed: {err:#}");
        OverdrawRefused
    })?
    else {
        return refused; // unknown key: no capacity, fail closed
    };
    let Some(family) = resolve_family(db, &owner, Some(bucket))
        .await
        .map_err(|err| {
            tracing::error!("bucket capacity reserve resolve failed: {err:#}");
            OverdrawRefused
        })?
    else {
        return refused; // no position row: nothing contributed, fail closed
    };
    match reserve_for_owner(db, &owner, &family, estimate).await {
        Ok(true) => Ok(()),
        Ok(false) => refused,
        Err(err) => {
            tracing::error!("bucket capacity reserve failed: {err:#}");
            refused
        }
    }
}

/// Reserves estimated burn against the depositor's remaining capacity in
/// the bucket, BEFORE the upstream call (the A-020 reserve step, bucket
/// edition). Refusal ⇒ the request never dials upstream. Single-bucket
/// keys only (multi-bucket keys take the `_for` form above).
pub async fn reserve_bucket_capacity(
    db: &Db,
    provider_key_id: &str,
    estimate: MicroUsd,
) -> Result<(), OverdrawRefused> {
    let refused = Err(OverdrawRefused);
    let Some(owner) = key_owner(db, provider_key_id).await.map_err(|err| {
        tracing::error!("bucket capacity reserve lookup failed: {err:#}");
        OverdrawRefused
    })?
    else {
        return refused;
    };
    let Some(family) = resolve_family(db, &owner, None).await.map_err(|err| {
        tracing::error!("bucket capacity reserve resolve failed: {err:#}");
        OverdrawRefused
    })?
    else {
        return refused;
    };
    match reserve_for_owner(db, &owner, &family, estimate).await {
        Ok(true) => Ok(()),
        Ok(false) => refused,
        Err(err) => {
            tracing::error!("bucket capacity reserve failed: {err:#}");
            refused
        }
    }
}

/// Settles a reservation at response end with the ACTUAL metered burn
/// (A-020 settle: estimate → truth). On error/cancel paths the caller
/// releases instead (capacity returns, nothing attributes). Bucket-aware
/// form used by the chat path.
pub async fn settle_bucket_capacity_for(
    db: &Db,
    provider_key_id: &str,
    bucket: &Bucket,
    estimate: MicroUsd,
    actual: MicroUsd,
) -> sqlx::Result<()> {
    let Some(owner) = key_owner(db, provider_key_id).await? else {
        return Ok(()); // key vanished mid-flight: nothing to adjust
    };
    let Some(family) = resolve_family(db, &owner, Some(bucket)).await? else {
        return Ok(());
    };
    adjust_for_owner(db, &owner, &family, actual.micros() - estimate.micros()).await
}

/// Settles a reservation at response end with the ACTUAL metered burn
/// (A-020 settle: estimate → truth). On error/cancel paths the caller
/// releases instead (capacity returns, nothing attributes). Single-bucket
/// keys only (multi-bucket keys take the `_for` form above).
pub async fn settle_bucket_capacity(
    db: &Db,
    provider_key_id: &str,
    estimate: MicroUsd,
    actual: MicroUsd,
) -> sqlx::Result<()> {
    let Some(owner) = key_owner(db, provider_key_id).await? else {
        return Ok(());
    };
    let Some(family) = resolve_family(db, &owner, None).await? else {
        return Ok(());
    };
    adjust_for_owner(db, &owner, &family, actual.micros() - estimate.micros()).await
}

/// Releases a reservation on error/cancel (A-020 release): the estimate
/// returns to the bucket, no attribution is written. Bucket-aware form.
pub async fn release_bucket_capacity_for(
    db: &Db,
    provider_key_id: &str,
    bucket: &Bucket,
    estimate: MicroUsd,
) -> sqlx::Result<()> {
    let Some(owner) = key_owner(db, provider_key_id).await? else {
        return Ok(());
    };
    let Some(family) = resolve_family(db, &owner, Some(bucket)).await? else {
        return Ok(());
    };
    adjust_for_owner(db, &owner, &family, -estimate.micros()).await
}

/// Releases a reservation on error/cancel (A-020 release): the estimate
/// returns to the bucket, no attribution is written. Single-bucket keys
/// only (multi-bucket keys take the `_for` form above).
pub async fn release_bucket_capacity(
    db: &Db,
    provider_key_id: &str,
    estimate: MicroUsd,
) -> sqlx::Result<()> {
    let Some(owner) = key_owner(db, provider_key_id).await? else {
        return Ok(());
    };
    let Some(family) = resolve_family(db, &owner, None).await? else {
        return Ok(());
    };
    adjust_for_owner(db, &owner, &family, -estimate.micros()).await
}
