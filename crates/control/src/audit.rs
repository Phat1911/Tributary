//! Audit log: every control-plane mutation leaves a trail (SPEC §6, M3).
//!
//! SECURITY INVARIANT (director directive 2026-10-17): `before`/`after` JSON
//! must NEVER contain key material. The only way rows get written is
//! [`record`], which takes [`AuditEntry`] snapshots built from the API's
//! masked view types — there is no code path from a raw secret to this
//! table. Pool keys log their display prefix, provider keys their masked
//! `key_prefix`, sessions nothing but ids.

use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use tributary_common::db::{Db, new_id, now_ms};

/// What kind of thing a mutation touched (stored per row for filtering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Org,
    Pool,
    Member,
    PoolKey,
    ProviderKey,
    QuotaRule,
    Session,
    ReviewFlag,
    ClassifierConfig,
}

impl TargetKind {
    /// Canonical wire form stored in `audit_log.target_kind`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Org => "org",
            Self::Pool => "pool",
            Self::Member => "member",
            Self::PoolKey => "pool_key",
            Self::ProviderKey => "provider_key",
            Self::QuotaRule => "quota_rule",
            Self::Session => "session",
            Self::ReviewFlag => "review_flag",
            Self::ClassifierConfig => "classifier_config",
        }
    }
}

/// One sanitized audit write. `before`/`after` are ALREADY masked snapshots
/// (built from `Public*` view structs or explicit display strings) — this
/// type deliberately has no way to carry a raw secret.
#[derive(Debug, Clone)]
pub struct AuditEntry<'a> {
    /// Org whose trail this row belongs to (`None` = host-instance scope).
    pub org_id: Option<&'a str>,
    pub actor_id: &'a str,
    pub actor_email: &'a str,
    /// `'<target>.<verb>'`, e.g. `member.created`.
    pub action: &'a str,
    pub target_kind: TargetKind,
    pub target_id: &'a str,
    /// Masked snapshot before the change (creates pass `None`).
    pub before: Option<Value>,
    /// Masked snapshot after the change (deletes pass `None`).
    pub after: Option<Value>,
    /// Client IP as seen by the control plane (already extracted upstream).
    pub ip: Option<&'a str>,
}

/// Writes one audit row. Failures are logged, not propagated: by the time an
/// audit trail misses is visible, the mutation itself has usually already
/// succeeded and failing the response would invite unsafe retries (same
/// posture as metering, AUDIT A-003). The integration tests prove the happy
/// path writes rows.
pub async fn record(db: &Db, entry: AuditEntry<'_>) {
    // Serialize the masked snapshots once; NULL columns for absent sides.
    let before_json = entry.before.map(|v| v.to_string());
    let after_json = entry.after.map(|v| v.to_string());
    let id = new_id();
    let ts = now_ms();
    let sql = db.sql(
        "INSERT INTO audit_log \
         (id, org_id, actor_id, actor_email, action, target_kind, target_id, \
          before_json, after_json, ip, ts) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&id)
        .bind(entry.org_id)
        .bind(entry.actor_id)
        .bind(entry.actor_email)
        .bind(entry.action)
        .bind(entry.target_kind.as_str())
        .bind(entry.target_id)
        .bind(&before_json)
        .bind(&after_json)
        .bind(entry.ip)
        .bind(ts)
        .execute(db.pool())
        .await;
    if let Err(err) = result {
        // Structured error log: detectable in ops, invisible to clients.
        tracing::error!(
            "audit insert failed (action={} target={}/{}): {err:#}",
            entry.action,
            entry.target_kind.as_str(),
            entry.target_id
        );
    }
}

/// One stored `audit_log` row, as served by the API.
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — the audit viewer's row shape
pub struct AuditRow {
    pub id: String,
    pub org_id: Option<String>,
    pub actor_id: String,
    pub actor_email: String,
    pub action: String,
    pub target_kind: String,
    pub target_id: String,
    /// Parsed masked snapshot (NULL column → JSON null, never a string).
    pub before: Value,
    pub after: Value,
    pub ip: Option<String>,
    /// Unix epoch millis (dashboard renders the local format).
    pub ts: i64,
}

/// Lists audit rows newest-first, paginated, scoped by org.
///
/// `org_scope = None` (host_admin) sees everything; `Some(org)` (org_admin)
/// sees only rows recorded against their org — the scope decision was made
/// at WRITE time, so reads never resolve targets.
pub async fn list(
    db: &Db,
    org_scope: Option<&str>,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<AuditRow>> {
    let (sql, bind_org) = if org_scope.is_some() {
        (
            "SELECT id, org_id, actor_id, actor_email, action, target_kind, target_id, \
              before_json, after_json, ip, ts \
             FROM audit_log WHERE org_id = ? ORDER BY ts DESC, id LIMIT ? OFFSET ?",
            true,
        )
    } else {
        (
            "SELECT id, org_id, actor_id, actor_email, action, target_kind, target_id, \
              before_json, after_json, ip, ts \
             FROM audit_log ORDER BY ts DESC, id LIMIT ? OFFSET ?",
            false,
        )
    };
    let sql = db.sql(sql);
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if bind_org {
        q = q.bind(org_scope.unwrap());
    }
    let rows = q
        .bind(limit)
        .bind(offset)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(audit_row_from_row)
        .collect();
    Ok(rows)
}

/// Maps a SELECT row into an [`AuditRow`] (before/after parsed to JSON
/// values; a corrupt column degrades to JSON null rather than 500-ing the
/// whole viewer).
fn audit_row_from_row(row: sqlx::any::AnyRow) -> AuditRow {
    let parse = |raw: Option<String>| match raw {
        Some(s) => serde_json::from_str(&s).unwrap_or(Value::Null),
        None => Value::Null,
    };
    AuditRow {
        id: row.get("id"),
        org_id: row.get("org_id"),
        actor_id: row.get("actor_id"),
        actor_email: row.get("actor_email"),
        action: row.get("action"),
        target_kind: row.get("target_kind"),
        target_id: row.get("target_id"),
        before: parse(row.get("before_json")),
        after: parse(row.get("after_json")),
        ip: row.get("ip"),
        ts: row.get("ts"),
    }
}

/// Loads every row as a raw string — used ONLY by the sanitization test to
/// prove no raw secret (or even a `key_hash`/`ciphertext` column name) ever
/// reached the table. Not exposed over the API.
pub async fn raw_rows_for_test(db: &Db) -> Vec<String> {
    let sql = db.sql(
        "SELECT id, org_id, actor_id, actor_email, action, target_kind, target_id, \
          before_json, after_json, ip, ts FROM audit_log",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .fetch_all(db.pool())
        .await
        .expect("audit select");
    rows.iter()
        .map(|r| {
            let text_cols = [
                "id",
                "org_id",
                "actor_id",
                "actor_email",
                "action",
                "target_kind",
                "target_id",
                "before_json",
                "after_json",
                "ip",
            ];
            let mut parts: Vec<String> = text_cols
                .iter()
                .map(|c| {
                    let v: Option<String> = r.get(c);
                    v.unwrap_or_default()
                })
                .collect();
            // ts is BIGINT — decode as i64, then stringify for the scan.
            let ts: i64 = r.get("ts");
            parts.push(ts.to_string());
            parts.join("|")
        })
        .collect()
}
