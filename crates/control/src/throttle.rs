//! Login throttling — closes AUDIT A-004 (per-email+IP, fixed window).
//!
//! Same layering as the gateway quota engine (D9: in-process fast path →
//! authoritative DB row): a hot attacker is rejected from the in-process
//! counter alone after the DB has confirmed the bucket once; a restart or
//! second process falls back to the `login_throttle` row, so the limit can
//! never be talked up. Buckets key on `(email, ip)` exactly as specified —
//! one attacker IP spraying many emails is an M4 concern (impossible-travel
//! / anomaly work), not this fix.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::Row;
use tributary_common::db::Db;

/// Fixed-window size: 15 minutes, UTC-aligned (multiples of the epoch —
/// same alignment idea as `tributary_common::time::period_start_ms`, just a
/// quarter-hour granularity the quota periods don't offer).
pub const WINDOW_MS: i64 = 15 * 60 * 1000;

/// The throttle's verdict for one login attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThrottleDecision {
    /// Under the limit — proceed to credential verification.
    Allow,
    /// Bucket is exhausted; the login must not even hash the password.
    Deny {
        /// Whole seconds until the window (and the bucket) resets.
        retry_after_secs: i64,
    },
}

/// Start of the fixed window containing `now` (UTC-aligned).
fn window_start(now_ms: i64) -> i64 {
    now_ms - now_ms.rem_euclid(WINDOW_MS)
}

/// In-process bucket state: `bucket → (window_start, fails)`. The fast path
/// mirrors the gateway rate counters — one lock, tiny maps, pruned on write.
#[derive(Default)]
struct Inner {
    buckets: HashMap<String, (i64, i64)>,
}

/// Cloneable handle held by [`crate::ControlState`].
#[derive(Clone, Default)]
pub struct LoginThrottle {
    inner: Arc<Mutex<Inner>>,
}

impl LoginThrottle {
    /// Fresh throttle (per ControlState / process).
    pub fn new() -> Self {
        Self::default()
    }

    /// Admission check before ANY password work (argond2 is expensive — a
    /// flooded login endpoint must not become a CPU DoS).
    pub async fn check(
        &self,
        db: &Db,
        email: &str,
        ip: &str,
        max_fails: i64,
        now: i64,
    ) -> sqlx::Result<ThrottleDecision> {
        let bucket = bucket_key(email, ip);
        let current = window_start(now);
        // Fast path: the in-proc counter alone may reject.
        if let Some((start, fails)) = self.proc_count(&bucket)
            && start == current
            && fails >= max_fails
        {
            return Ok(deny(now));
        }
        // Authoritative: the DB row (restart/multi-process backstop).
        if let Some((start, fails)) = fetch_bucket(db, &bucket).await?
            && start == current
            && fails >= max_fails
        {
            return Ok(deny(now));
        }
        Ok(ThrottleDecision::Allow)
    }

    /// Records one FAILED attempt (both layers). Successful logins call
    /// [`Self::clear`] instead — a legit user's typos must not lock them out.
    pub async fn record_failure(&self, db: &Db, email: &str, ip: &str, now: i64) {
        let bucket = bucket_key(email, ip);
        let current = window_start(now);
        if let Err(err) = upsert_bucket(db, &bucket, current).await {
            // If the write fails we still bumped the in-proc counter, so the
            // limit holds for this process; the DB row catches up next fail.
            tracing::error!("login throttle write failed for bucket: {err:#}");
        }
        let mut inner = self.inner.lock().expect("throttle lock");
        let (start, fails) = inner.buckets.get(&bucket).copied().unwrap_or((0, 0));
        let fails = if start == current { fails + 1 } else { 1 };
        inner.buckets.insert(bucket, (current, fails));
        // Opportunistic pruning (bounded memory, same as the rate counters).
        inner
            .buckets
            .retain(|_, (s, _)| now.saturating_sub(*s) < WINDOW_MS);
    }

    /// Clears the bucket after a SUCCESSFUL login (self-service recovery).
    pub async fn clear(&self, db: &Db, email: &str, ip: &str) {
        let bucket = bucket_key(email, ip);
        if let Err(err) = delete_bucket(db, &bucket).await {
            tracing::error!("login throttle clear failed: {err:#}");
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.buckets.remove(&bucket);
        }
    }

    /// Current in-process counter for one bucket.
    fn proc_count(&self, bucket: &str) -> Option<(i64, i64)> {
        let inner = self.inner.lock().expect("throttle lock");
        inner.buckets.get(bucket).copied()
    }
}

/// Builds the Deny verdict (seconds until this window ends).
fn deny(now: i64) -> ThrottleDecision {
    let reset = window_start(now) + WINDOW_MS;
    // Round up to whole seconds (reset > now always, so plain math is fine —
    // `div_ceil` reads better but stays behind an unstable gate on the
    // current toolchain).
    ThrottleDecision::Deny {
        retry_after_secs: ((reset - now) + 999) / 1000,
    }
}

/// Storage key: `'<email>|<ip>'` — the per-email+IP bucket A-004 specifies.
fn bucket_key(email: &str, ip: &str) -> String {
    format!("{email}|{ip}")
}

/// Reads one bucket row from the DB (`None` when absent/expired).
async fn fetch_bucket(db: &Db, bucket: &str) -> sqlx::Result<Option<(i64, i64)>> {
    let sql = db.sql("SELECT window_start, fails FROM login_throttle WHERE bucket = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(bucket)
        .fetch_optional(db.pool())
        .await?;
    Ok(row.map(|r| (r.get("window_start"), r.get("fails"))))
}

/// Insert-or-advance the bucket: on window rollover the count restarts at 1.
/// Read-then-write inside one transaction (portable upsert, same trick as
/// `upsert_quota_rule` — delete+insert, avoiding ON CONFLICT dialect
/// differences). Worst case two racing failures undercount by one under
/// read-committed isolation; the limit boundary is not a security cliff and
/// the in-process layer counts independently.
async fn upsert_bucket(db: &Db, bucket: &str, current: i64) -> sqlx::Result<()> {
    let mut tx = db.pool().begin().await?;
    let sel = db.sql("SELECT window_start, fails FROM login_throttle WHERE bucket = ?");
    let existing: Option<(i64, i64)> = sqlx::query(sqlx::AssertSqlSafe(sel.as_str()))
        .bind(bucket)
        .fetch_optional(&mut *tx)
        .await?
        .map(|r| {
            let start: i64 = r.get("window_start");
            let fails: i64 = r.get("fails");
            (start, fails)
        });
    let fails = match existing {
        Some((start, fails)) if start == current => fails + 1,
        _ => 1, // first failure of this window, or the window rolled over
    };
    let del = db.sql("DELETE FROM login_throttle WHERE bucket = ?");
    sqlx::query(sqlx::AssertSqlSafe(del.as_str()))
        .bind(bucket)
        .execute(&mut *tx)
        .await?;
    let ins = db.sql("INSERT INTO login_throttle (bucket, window_start, fails) VALUES (?, ?, ?)");
    sqlx::query(sqlx::AssertSqlSafe(ins.as_str()))
        .bind(bucket)
        .bind(current)
        .bind(fails)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Deletes one bucket row.
async fn delete_bucket(db: &Db, bucket: &str) -> sqlx::Result<()> {
    let sql = db.sql("DELETE FROM login_throttle WHERE bucket = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(bucket)
        .execute(db.pool())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_align_to_quarter_hours() {
        assert_eq!(window_start(0), 0);
        assert_eq!(window_start(WINDOW_MS - 1), 0);
        assert_eq!(window_start(WINDOW_MS), WINDOW_MS);
        assert_eq!(window_start(WINDOW_MS + 1), WINDOW_MS);
    }

    #[test]
    fn deny_reports_positive_retry_after() {
        let ThrottleDecision::Deny { retry_after_secs } = deny(WINDOW_MS - 2500) else {
            panic!("expected deny");
        };
        assert_eq!(retry_after_secs, 3);
    }

    #[test]
    fn bucket_keys_combine_email_and_ip() {
        assert_eq!(bucket_key("a@b.co", "1.2.3.4"), "a@b.co|1.2.3.4");
    }
}
