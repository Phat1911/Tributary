//! Admin four-eyes for sensitive actions (D33, 2026-10-21).
//!
//! The rule set, verbatim from the decision:
//! - ROUTINE ops (own members/keys/budgets) stay single-admin + audit.
//! - SENSITIVE actions need **2 distinct admins** — actor requests, a
//!   different admin (org admin of the same org, or the host) approves.
//!   The sensitive set is EXACTLY: demote/remove another admin; alter
//!   another admin's key (expiry, revoke); org-wide policy (classifier
//!   purpose/auto-block config, pool deletion).
//! - Containment exemption: a key WITH an open system alert (classifier
//!   flag, D32) may be suspended/expired/revoked by a single admin — the
//!   alert ID is linked in the audit row. System-initiated suspensions
//!   (classifier auto-block) never touch approvals at all.
//! - The last remaining admin can never be demoted/removed.
//! - Single-admin orgs auto-pass, with a logged marker (never deadlock).
//!
//! Secrets never ride in a pending payload: the classifier key is sealed
//! at REQUEST time and the sealed blob defers (rule 2 posture).

use serde_json::Value;
use serde_json::json;

use tributary_common::db::new_id;

use crate::db::{self};
use tributary_common::db::Db;

/// The D33 sensitive-action kinds. `as_str` is the wire/DB form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveKind {
    /// Demote another org admin to member.
    AdminDemote,
    /// Set/clear expiry on a key owned by another admin.
    AdminKeyExpiry,
    /// Revoke a key owned by another admin.
    AdminKeyRevoke,
    /// Org-wide policy: classifier configure / remove / go-live.
    OrgPolicy,
    /// Disable (delete) a pool.
    PoolDisable,
}

impl SensitiveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SensitiveKind::AdminDemote => "admin_demote",
            SensitiveKind::AdminKeyExpiry => "admin_key_expiry",
            SensitiveKind::AdminKeyRevoke => "admin_key_revoke",
            SensitiveKind::OrgPolicy => "org_policy",
            SensitiveKind::PoolDisable => "pool_disable",
        }
    }

    /// Human label for the review queue.
    pub fn label(self) -> &'static str {
        match self {
            SensitiveKind::AdminDemote => "Demote admin",
            SensitiveKind::AdminKeyExpiry => "Set expiry on admin key",
            SensitiveKind::AdminKeyRevoke => "Revoke admin key",
            SensitiveKind::OrgPolicy => "Org policy change",
            SensitiveKind::PoolDisable => "Disable pool",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "admin_demote" => Some(SensitiveKind::AdminDemote),
            "admin_key_expiry" => Some(SensitiveKind::AdminKeyExpiry),
            "admin_key_revoke" => Some(SensitiveKind::AdminKeyRevoke),
            "org_policy" => Some(SensitiveKind::OrgPolicy),
            "pool_disable" => Some(SensitiveKind::PoolDisable),
            _ => None,
        }
    }
}

/// The request-time gate verdict.
#[derive(Debug, PartialEq)]
pub enum Gate {
    /// Execute immediately. `solo` = single-admin org auto-pass (D33's
    /// never-deadlock rule) — the audit row carries the marker.
    ExecuteDirectly { solo: bool },
    /// Defer: create a pending action, await a distinct admin.
    RequiresApproval,
}

/// Evaluates the D33 gate for an org: ≥2 org admins ⇒ dual-control; the
/// lone admin executes directly with a logged marker.
pub async fn gate_for(db: &Db, org_id: &str) -> anyhow::Result<Gate> {
    let admins = db::org_admin_count(db, org_id)
        .await
        .map_err(|e| anyhow::anyhow!("admin count: {e}"))?;
    Ok(if admins >= 2 {
        Gate::RequiresApproval
    } else {
        Gate::ExecuteDirectly { solo: true }
    })
}

/// Containment check (D33 exemption): an admin may act SOLO on a key that
/// has an open system alert. Returns the linked flag id for the audit row
/// (the audit MUST link the alert — a solo action without one is the
/// detectable policy violation).
pub async fn linked_alert_for_key(db: &Db, pool_key_id: &str) -> anyhow::Result<Option<String>> {
    db::latest_open_flag_for_key(db, pool_key_id)
        .await
        .map_err(|e| anyhow::anyhow!("linked alert lookup: {e}"))
}

/// Records one pending action (the request half of the two-step API).
/// Returns the row id. The caller audits `approval.requested` around it.
pub async fn create_pending(
    db: &Db,
    org_id: &str,
    kind: SensitiveKind,
    target_id: &str,
    payload: &Value,
    requested_by: &str,
    now: i64,
) -> anyhow::Result<String> {
    let id = new_id();
    db::insert_pending_action(
        db,
        &id,
        org_id,
        kind.as_str(),
        target_id,
        &payload.to_string(),
        requested_by,
        now,
    )
    .await
    .map_err(|e| anyhow::anyhow!("insert pending action: {e}"))?;
    Ok(id)
}

/// Executes an approved pending action. Re-validates every guard against
/// CURRENT state (the world may have moved between request and approval):
/// the demote target must still be an admin and must not be the last one;
/// key/pool targets must still exist. Returns a summary for the executed
/// action's audit row.
pub async fn execute(db: &Db, action: &db::PendingActionRow, now: i64) -> anyhow::Result<Value> {
    let payload: Value = serde_json::from_str(&action.payload_json).unwrap_or(Value::Null);
    match SensitiveKind::parse(&action.kind) {
        Some(SensitiveKind::AdminDemote) => {
            let target_id = payload["user_id"].as_str().unwrap_or(&action.target_id);
            // Last-admin guard AT EXECUTION (request-time can be stale).
            let admins = db::org_admin_count(db, &action.org_id).await?;
            if admins <= 1 {
                anyhow::bail!("refused: target is the last remaining admin");
            }
            let affected = db::demote_org_admin(db, target_id).await?;
            if affected == 0 {
                anyhow::bail!("refused: target is no longer an admin");
            }
            Ok(json!({
                "user_id": target_id,
                "role_after": "member",
            }))
        }
        Some(SensitiveKind::AdminKeyExpiry) => {
            let key_id = payload["key_id"].as_str().unwrap_or(&action.target_id);
            let expires_at = payload["expires_at"].as_i64();
            db::set_pool_key_expiry(db, key_id, expires_at).await?;
            Ok(json!({ "key_id": key_id, "expires_at": expires_at }))
        }
        Some(SensitiveKind::AdminKeyRevoke) => {
            let key_id = payload["key_id"].as_str().unwrap_or(&action.target_id);
            let affected = db::revoke_pool_key(db, key_id).await?;
            if affected == 0 {
                anyhow::bail!("refused: key already revoked or gone");
            }
            Ok(json!({ "key_id": key_id, "status": "revoked" }))
        }
        Some(SensitiveKind::OrgPolicy) => match payload["policy_action"].as_str() {
            Some("configure") => {
                // The classifier row was built (and its key SEALED) at
                // request time; execution only refreshes the timestamps —
                // entering auto_block starts its shadow window NOW.
                let mut row = classifier_row_from_payload(&payload);
                row.updated_at = now;
                row.shadow_started_at = if row.mode == "auto_block" {
                    Some(now)
                } else {
                    None
                };
                db::upsert_classifier_config(db, &row).await?;
                Ok(json!({
                    "policy": "classifier_configured",
                    "dialect": row.dialect,
                    "model": row.model,
                    "mode": row.mode,
                }))
            }
            Some("remove") => {
                db::delete_classifier_config(db, &action.org_id).await?;
                Ok(json!({ "policy": "classifier_removed" }))
            }
            Some("go_live") => {
                let affected = db::classifier_go_live(db, &action.org_id, now).await?;
                if affected == 0 {
                    anyhow::bail!("refused: no auto_block config to go live");
                }
                Ok(json!({ "policy": "classifier_go_live" }))
            }
            other => anyhow::bail!("unknown policy action: {other:?}"),
        },
        Some(SensitiveKind::PoolDisable) => {
            let pool_id = payload["pool_id"].as_str().unwrap_or(&action.target_id);
            db::disable_pool(db, pool_id).await?;
            Ok(json!({ "pool_id": pool_id, "status": "disabled" }))
        }
        None => anyhow::bail!("unknown pending action kind {}", action.kind),
    }
}

/// Rebuilds a `ClassifierConfigRow` from the sealed payload stored at
/// request time. Field-by-field (never `Deserialize` on the row itself —
/// the payload is OUR sanitized construction, and staying explicit keeps
/// a tampered payload from inventing fields).
fn classifier_row_from_payload(payload: &Value) -> db::ClassifierConfigRow {
    db::ClassifierConfigRow {
        org_id: payload["org_id"].as_str().unwrap_or_default().to_string(),
        base_url: payload["base_url"].as_str().unwrap_or_default().to_string(),
        dialect: payload["dialect"].as_str().unwrap_or("custom").to_string(),
        model: payload["model"].as_str().unwrap_or_default().to_string(),
        ciphertext: payload["ciphertext"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        dek_wrapped: payload["dek_wrapped"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        key_prefix: payload["key_prefix"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        purpose_statement: payload["purpose_statement"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        mode: payload["mode"].as_str().unwrap_or("alert").to_string(),
        alert_threshold_bp: payload["alert_threshold_bp"].as_i64().unwrap_or(5000),
        block_threshold_bp: payload["block_threshold_bp"].as_i64().unwrap_or(1500),
        auto_block_live: false,
        shadow_started_at: None,
        sampling_per_hour: payload["sampling_per_hour"].as_i64().unwrap_or(30),
        updated_at: 0,
    }
}

/// Sanitizes a classifier row into the deferrable payload (sealed blobs
/// only — the plaintext key existed solely at request time).
pub fn classifier_payload(row: &db::ClassifierConfigRow, policy_action: &str) -> Value {
    json!({
        "policy_action": policy_action,
        "org_id": row.org_id,
        "base_url": row.base_url,
        "dialect": row.dialect,
        "model": row.model,
        "ciphertext": row.ciphertext,
        "dek_wrapped": row.dek_wrapped,
        "key_prefix": row.key_prefix,
        "purpose_statement": row.purpose_statement,
        "mode": row.mode,
        "alert_threshold_bp": row.alert_threshold_bp,
        "block_threshold_bp": row.block_threshold_bp,
        "sampling_per_hour": row.sampling_per_hour,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip() {
        for kind in [
            SensitiveKind::AdminDemote,
            SensitiveKind::AdminKeyExpiry,
            SensitiveKind::AdminKeyRevoke,
            SensitiveKind::OrgPolicy,
            SensitiveKind::PoolDisable,
        ] {
            assert_eq!(SensitiveKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(SensitiveKind::parse("nope"), None);
    }

    #[test]
    fn classifier_payload_round_trips_the_sealed_row() {
        let row = db::ClassifierConfigRow {
            org_id: "o1".into(),
            base_url: "https://api.typesafe.ai".into(),
            dialect: "jev".into(),
            model: "jev-latest".into(),
            ciphertext: "ct".into(),
            dek_wrapped: "dw".into(),
            key_prefix: "tsf…3456".into(),
            purpose_statement: "coding only".into(),
            mode: "auto_block".into(),
            alert_threshold_bp: 5000,
            block_threshold_bp: 1500,
            auto_block_live: false,
            shadow_started_at: None,
            sampling_per_hour: 30,
            updated_at: 1,
        };
        let payload = classifier_payload(&row, "configure");
        let back = classifier_row_from_payload(&payload);
        assert_eq!(back.org_id, row.org_id);
        assert_eq!(back.dialect, "jev");
        assert_eq!(back.ciphertext, "ct");
        assert_eq!(back.mode, "auto_block");
        // Never carried: execution-time fields reset by design.
        assert_eq!(back.updated_at, 0);
        assert!(back.shadow_started_at.is_none());
    }
}
