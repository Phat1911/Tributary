//! Quota & rate-limit engine — rule 4's enforcement point.
//!
//! SPEC §5 lifecycle step 3: "Rate-limit & budget check … BEFORE any
//! upstream call". Checks run in [`check`] after auth and BEFORE the
//! provider key is even decrypted — a denied request costs zero upstream
//! tokens and one indexed DB aggregate.
//!
//! D9 layering ("Postgres + in-process cache", a.k.a. in-proc TTL cache →
//! authoritative DB row):
//! - **Rules** (rarely change) are cached per subject for 5 s in-process.
//! - **Request-rate** (per minute) keeps an in-process fixed-window counter
//!   as the fast path — hot abusers are rejected without touching the DB —
//!   while the DB remains authoritative (COUNT over `usage_events`), so a
//!   restart or multi-process setup cannot be talked into a bigger budget.
//! - **Token quotas** (hour/day/month) aggregate `usage_events` directly:
//!   metering rows are the source of truth, written exactly once per served
//!   request by the chat path.
//!
//! Semantics: usage rows exist only for requests that REACHED upstream, so
//! denied requests never consume quota — exhaustion sticks until the window
//! resets. Multiple rules AND together (most restrictive wins).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tributary_common::db::Db;
use tributary_common::time::{Period, next_period_start_ms, period_start_ms};

/// How long a fetched rule set stays valid in-process (SPEC: 1–5 s).
const RULES_TTL: Duration = Duration::from_secs(5);
/// Rate buckets older than this are pruned on write (bounded memory).
const RATE_BUCKET_RETENTION_MS: i64 = 120_000;
/// At most this often, an admission attempt runs the reservation TTL
/// sweep (A-020) — one indexed DELETE per cadence window, not per request.
const RESERVATION_SWEEP_EVERY: Duration = Duration::from_secs(5);
/// Completion-token estimate when the client declares no `max_tokens`
/// (A-020). Deliberately modest: large enough that unbounded generations
/// cannot stream for free past a cap, small enough that ordinary traffic
/// near a cap is not false-denied. Requests may still overshoot by one
/// request's estimate — that is the documented reservation semantics.
const DEFAULT_COMPLETION_ESTIMATE_TOKENS: i64 = 256;

/// A request's pessimistic token footprint (A-020): estimated prompt
/// (≈4 chars/token over message content) + the client-declared completion
/// cap, or the default. Cost-unit reservation joins with the D23 price
/// table; today the enforced units are tokens and requests.
pub fn estimate_tokens(body: &serde_json::Value) -> i64 {
    let prompt_chars: usize = body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .map(|m| {
                    m.get("content")
                        .and_then(serde_json::Value::as_str)
                        .map(str::len)
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0);
    // Ceil-divide: a 1-char prompt is still one token.
    let prompt_est = ((prompt_chars as i64) + 3) / 4;
    let completion_cap = body
        .get("max_completion_tokens")
        .or_else(|| body.get("max_tokens"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(DEFAULT_COMPLETION_ESTIMATE_TOKENS);
    prompt_est.max(1) + completion_cap.max(0)
}

/// A live admitted request's reservations (A-020). Dropped by value; the
/// settle/release call is explicit on the chat path (this is not a Drop
/// guard because streaming settles from a spawned task, and implicit
/// async-on-drop is not a thing we can do safely).
#[derive(Debug, Clone, Default)]
pub struct Reservation {
    pub buckets: Vec<crate::db::ReservedBucket>,
}

/// One `quota_rules` row as the engine sees it.
#[derive(Debug, Clone)]
pub struct QuotaRule {
    pub subject_kind: String,
    pub subject_id: String,
    pub period: String,
    pub limit_tokens: Option<i64>,
    pub limit_requests: Option<i64>,
}

/// Who is making a request — every subject kind that can carry rules.
#[derive(Debug, Clone)]
pub struct QuotaSubject {
    pub pool_key_id: String,
    pub user_id: Option<String>,
    pub pool_id: String,
}

impl QuotaSubject {
    /// The (kind, id) pairs rules can attach to, most specific first.
    fn keys(&self) -> Vec<(&'static str, &str)> {
        let mut keys = vec![("pool_key", self.pool_key_id.as_str())];
        if let Some(user) = self.user_id.as_deref() {
            keys.push(("user", user));
        }
        keys.push(("pool", self.pool_id.as_str()));
        keys
    }
}

/// The engine's verdict for one admission check.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Allow,
    Deny {
        /// Human-facing reason (limit size and window).
        message: String,
        /// Epoch ms when the limiting window resets.
        reset_at_ms: i64,
    },
}

/// Mutable engine state, shared behind one lock (single binary, low
/// contention — SPEC §13 keeps rate limiting in-process by design).
#[derive(Default)]
struct Inner {
    /// subject "kind:id" → (fetched_at, rules). The in-proc TTL cache.
    rules_cache: HashMap<String, (Instant, Vec<QuotaRule>)>,
    /// (subject "kind:id", minute-bucket-ms) → served request count.
    rate_counters: HashMap<(String, i64), u64>,
    /// Last TTL-sweep run (A-020; rate-limited to one per cadence).
    last_sweep: Option<Instant>,
}

/// Cloneable handle held by [`crate::state::AppState`].
#[derive(Clone, Default)]
pub struct QuotaEngine {
    inner: Arc<Mutex<Inner>>,
}

impl QuotaEngine {
    /// Fresh engine (per AppState / process).
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs every applicable rule for the subject (rule 4 admission gate).
    pub async fn check(
        &self,
        db: &Db,
        subject: &QuotaSubject,
        now_ms: i64,
    ) -> sqlx::Result<Decision> {
        for (kind, id) in subject.keys() {
            let rules = self.rules_for(db, kind, id).await?;
            for rule in rules {
                if let Some(limit) = rule.limit_requests {
                    let period = Period::parse(&rule.period).unwrap_or(Period::Minute);
                    let window_start = period_start_ms(period, now_ms);
                    // Fast path: in-process counter alone may reject.
                    let counted = self.rate_count(kind, id, window_start);
                    let served = if (counted as i64) >= limit {
                        counted as i64
                    } else {
                        // Authoritative: metered rows in this window.
                        usage_count(db, kind, id, window_start).await?
                    };
                    if served >= limit {
                        let reset = next_period_start_ms(period, now_ms);
                        return Ok(deny_decision(limit, period, reset, false));
                    }
                }
                if let Some(limit) = rule.limit_tokens {
                    let period = Period::parse(&rule.period).unwrap_or(Period::Day);
                    let window_start = period_start_ms(period, now_ms);
                    let used = usage_tokens(db, kind, id, window_start).await?;
                    if used >= limit {
                        let reset = next_period_start_ms(period, now_ms);
                        return Ok(deny_decision(limit, period, reset, true));
                    }
                }
            }
        }
        Ok(Decision::Allow)
    }

    /// Records an admitted request in the in-process rate counters.
    ///
    /// Called exactly once per request that proceeds to upstream — the DB
    /// side of the counters is the usage row the chat path writes.
    pub fn record_request(&self, subject: &QuotaSubject, now_ms: i64) {
        let bucket = period_start_ms(Period::Minute, now_ms);
        let mut inner = self.inner.lock().expect("quota engine lock");
        for (kind, id) in subject.keys() {
            let key = format!("{kind}:{id}");
            *inner.rate_counters.entry((key, bucket)).or_insert(0) += 1;
        }
        // Opportunistic pruning: drop buckets from >2 minutes ago.
        inner
            .rate_counters
            .retain(|(_, b), _| now_ms.saturating_sub(*b) <= RATE_BUCKET_RETENTION_MS);
    }

    /// Rule-4 phase 2 (A-020): atomically RESERVE every applicable rule's
    /// window BEFORE the upstream call. `check` still runs first as the
    /// cheap deny; this closes the concurrent-burst TOCTOU (usage only
    /// lands at response end). Returns the reservation to settle/release,
    /// or the deny verdict when any window cannot absorb the estimate —
    /// with earlier reservations in THIS call already rolled back.
    pub async fn reserve(
        &self,
        db: &Db,
        subject: &QuotaSubject,
        estimate_tokens: i64,
        now_epoch_ms: i64,
    ) -> sqlx::Result<Result<Reservation, Decision>> {
        self.maybe_sweep(db).await;
        let mut reservation = Reservation::default();
        for (kind, id) in subject.keys() {
            let rules = self.rules_for(db, kind, id).await?;
            for rule in rules {
                if rule.limit_tokens.is_none() && rule.limit_requests.is_none() {
                    continue; // defensive: rules always carry one limit
                }
                let period = Period::parse(&rule.period).unwrap_or(Period::Day);
                let window_start = period_start_ms(period, now_epoch_ms);
                let bucket = crate::db::ReservedBucket {
                    subject_kind: kind.to_string(),
                    subject_id: id.to_string(),
                    period: rule.period.clone(),
                    window_start,
                    est_tokens: if rule.limit_tokens.is_some() {
                        estimate_tokens
                    } else {
                        0
                    },
                    est_requests: if rule.limit_requests.is_some() { 1 } else { 0 },
                };
                if crate::db::reserve_bucket(db, &bucket, rule.limit_tokens, rule.limit_requests)
                    .await?
                {
                    reservation.buckets.push(bucket);
                } else {
                    // Window full: unwind what THIS call already took so a
                    // multi-rule subject leaves no phantom reservations.
                    self.release(db, &reservation).await;
                    let reset = next_period_start_ms(period, now_epoch_ms);
                    let limit = rule
                        .limit_tokens
                        .unwrap_or_else(|| rule.limit_requests.unwrap_or(0));
                    return Ok(Err(deny_reservation_decision(
                        limit,
                        period,
                        reset,
                        rule.limit_tokens.is_some(),
                        estimate_tokens,
                    )));
                }
            }
        }
        Ok(Ok(reservation))
    }

    /// Ends a reservation (A-020 settle/release — the identical decrement;
    /// settle's usage row is written separately by the chat path before
    /// this runs). Best-effort per bucket: a failed decrement logs loudly
    /// and the TTL sweep reclaims it when the window rolls over.
    pub async fn release(&self, db: &Db, reservation: &Reservation) {
        for bucket in &reservation.buckets {
            if let Err(err) = crate::db::release_bucket(db, bucket).await {
                tracing::error!(
                    "reservation release failed for {}:{} {} (sweep will reclaim): {err:#}",
                    bucket.subject_kind,
                    bucket.subject_id,
                    bucket.period
                );
            }
        }
    }

    /// Runs the TTL sweep at most once per cadence window (A-020).
    async fn maybe_sweep(&self, db: &Db) {
        let due = {
            let Ok(inner) = self.inner.lock() else {
                return;
            };
            inner
                .last_sweep
                .is_none_or(|at| at.elapsed() >= RESERVATION_SWEEP_EVERY)
        };
        if !due {
            return;
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_sweep = Some(Instant::now());
        }
        if let Err(err) =
            crate::db::sweep_reservation_windows(db, tributary_common::db::now_ms()).await
        {
            tracing::warn!("reservation sweep failed (next cadence retries): {err:#}");
        }
    }

    /// Current in-process counter for (subject, window).
    fn rate_count(&self, kind: &str, id: &str, bucket_ms: i64) -> u64 {
        let inner = self.inner.lock().expect("quota engine lock");
        inner
            .rate_counters
            .get(&(format!("{kind}:{id}"), bucket_ms))
            .copied()
            .unwrap_or(0)
    }

    /// Cached rule fetch for one subject (DB on cache miss/expiry).
    async fn rules_for(&self, db: &Db, kind: &str, id: &str) -> sqlx::Result<Vec<QuotaRule>> {
        let cache_key = format!("{kind}:{id}");
        if let Ok(inner) = self.inner.lock()
            && let Some((fetched_at, rules)) = inner.rules_cache.get(&cache_key)
            && fetched_at.elapsed() < RULES_TTL
        {
            return Ok(rules.clone());
        }
        let rules = load_rules(db, kind, id).await?;
        if let Ok(mut inner) = self.inner.lock() {
            inner
                .rules_cache
                .insert(cache_key, (Instant::now(), rules.clone()));
        }
        Ok(rules)
    }
}

/// Helper building the Deny verdict (kept out of `check` for readability).
fn deny_decision(limit: i64, period: Period, reset_ms: i64, tokens: bool) -> Decision {
    let unit = if tokens { "tokens" } else { "requests" };
    let human = tributary_common::time::rfc3339_utc(reset_ms);
    Decision::Deny {
        message: format!(
            "Limit reached: {limit} {unit} per {}. Resets at {human}.",
            period.as_str()
        ),
        reset_at_ms: reset_ms,
    }
}

/// The A-020 reserve-path Deny — deliberately a DIFFERENT message from
/// [`deny_decision`] (director note 2026-10-24): a request refused because
/// its ESTIMATE cannot fit the remaining window is not the same event as a
/// cap exhausted on ACTUAL usage, and admins debugging tiny caps must be
/// able to tell them apart at a glance.
fn deny_reservation_decision(
    limit: i64,
    period: Period,
    reset_ms: i64,
    tokens: bool,
    estimate_tokens: i64,
) -> Decision {
    let human = tributary_common::time::rfc3339_utc(reset_ms);
    let message = if tokens {
        format!(
            "Estimated {estimate_tokens} tokens (prompt chars/4 + max_completion_tokens, \
             default 256) exceeds the remaining budget of the {limit}-token {} window. \
             Declare max_tokens to lower the estimate. Resets at {human}.",
            period.as_str()
        )
    } else {
        format!(
            "Request would exceed the {limit}-request {} window. Resets at {human}.",
            period.as_str()
        )
    };
    Decision::Deny {
        message,
        reset_at_ms: reset_ms,
    }
}

/// Loads rules for one subject from the DB (indexed by the UNIQUE key).
async fn load_rules(db: &Db, kind: &str, id: &str) -> sqlx::Result<Vec<QuotaRule>> {
    use sqlx::Row;
    let sql = db.sql(
        "SELECT subject_kind, subject_id, period, limit_tokens, limit_requests \
         FROM quota_rules WHERE subject_kind = ? AND subject_id = ?",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(kind)
        .bind(id)
        .fetch_all(db.pool())
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| QuotaRule {
            subject_kind: row.get("subject_kind"),
            subject_id: row.get("subject_id"),
            period: row.get("period"),
            limit_tokens: row.get("limit_tokens"),
            limit_requests: row.get("limit_requests"),
        })
        .collect())
}

/// The usage-events column a subject kind aggregates over.
pub(crate) fn usage_column(kind: &str) -> &'static str {
    match kind {
        "user" => "user_id",
        "pool" => "pool_id",
        _ => "pool_key_id",
    }
}

/// Authoritative served-request count in the window (DB).
async fn usage_count(db: &Db, kind: &str, id: &str, since_ms: i64) -> sqlx::Result<i64> {
    use sqlx::Row;
    let sql = db.sql(&format!(
        "SELECT COUNT(*) AS n FROM usage_events WHERE {} = ? AND ts >= ?",
        usage_column(kind)
    ));
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .bind(since_ms)
        .fetch_one(db.pool())
        .await?;
    let n: i64 = row.get("n");
    Ok(n)
}

/// Authoritative token total in the window (DB) — tokens = prompt+completion.
async fn usage_tokens(db: &Db, kind: &str, id: &str, since_ms: i64) -> sqlx::Result<i64> {
    use sqlx::Row;
    let sql = db.sql(&format!(
        "SELECT CAST(COALESCE(SUM(prompt_tokens + completion_tokens), 0) AS BIGINT) AS t \
         FROM usage_events WHERE {} = ? AND ts >= ?",
        usage_column(kind)
    ));
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .bind(since_ms)
        .fetch_one(db.pool())
        .await?;
    let t: i64 = row.get("t");
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a020_estimate_uses_declared_cap_and_default_fallback() {
        let body = serde_json::json!({
            "model": "m",
            "max_tokens": 18,
            "messages": [{"role": "user", "content": "hi"}], // 2 chars ⇒ 1 token
        });
        assert_eq!(estimate_tokens(&body), 19);
        // No declared cap ⇒ the documented default completion estimate.
        let body = serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
        });
        assert_eq!(
            estimate_tokens(&body),
            1 + DEFAULT_COMPLETION_ESTIMATE_TOKENS
        );
        // The newer OpenAI field name wins when both are present.
        let body = serde_json::json!({
            "model": "m",
            "max_completion_tokens": 7,
            "max_tokens": 999,
            "messages": [],
        });
        assert_eq!(estimate_tokens(&body), 8);
    }

    #[test]
    fn subject_keys_order_most_specific_first() {
        let subject = QuotaSubject {
            pool_key_id: "k1".into(),
            user_id: Some("u1".into()),
            pool_id: "p1".into(),
        };
        let keys = subject.keys();
        assert_eq!(keys[0], ("pool_key", "k1"));
        assert_eq!(keys[1], ("user", "u1"));
        assert_eq!(keys[2], ("pool", "p1"));
    }

    #[test]
    fn record_request_bumps_only_current_minute() {
        let engine = QuotaEngine::new();
        let subject = QuotaSubject {
            pool_key_id: "k1".into(),
            user_id: None,
            pool_id: "p1".into(),
        };
        let now = 1_792_222_316_000; // 2026-10-17T07:31:56Z
        engine.record_request(&subject, now);
        engine.record_request(&subject, now + 500);
        let bucket = period_start_ms(Period::Minute, now);
        assert_eq!(engine.rate_count("pool_key", "k1", bucket), 2);
        // A later minute starts from zero.
        assert_eq!(engine.rate_count("pool_key", "k1", bucket + 60_000), 0);
    }

    #[test]
    fn deny_message_carries_limit_window_and_reset() {
        let reset = 1_793_068_800_000; // some boundary
        let decision = deny_decision(500, Period::Day, reset, true);
        match decision {
            Decision::Deny {
                message,
                reset_at_ms,
            } => {
                assert!(message.contains("500 tokens per day"));
                assert!(message.contains("Resets at 2026-"));
                assert_eq!(reset_at_ms, reset);
            }
            _ => panic!("expected deny"),
        }
    }
}
