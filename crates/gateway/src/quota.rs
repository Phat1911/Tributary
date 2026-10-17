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
fn usage_column(kind: &str) -> &'static str {
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
