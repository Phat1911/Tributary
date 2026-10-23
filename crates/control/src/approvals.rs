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
    /// Rotate a key owned by another admin (replaces the credential —
    /// final-scan: rotation was an ungated bypass of AdminKeyRevoke).
    AdminKeyRotate,
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
            SensitiveKind::AdminKeyRotate => "admin_key_rotate",
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
            SensitiveKind::AdminKeyRotate => "Rotate admin key",
            SensitiveKind::OrgPolicy => "Org policy change",
            SensitiveKind::PoolDisable => "Disable pool",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "admin_demote" => Some(SensitiveKind::AdminDemote),
            "admin_key_expiry" => Some(SensitiveKind::AdminKeyExpiry),
            "admin_key_revoke" => Some(SensitiveKind::AdminKeyRevoke),
            "admin_key_rotate" => Some(SensitiveKind::AdminKeyRotate),
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

/// Builds the human-readable one-liner for the approvals queue. The
/// payload stays server-side; THIS is its sanitized projection — the
/// approver decides on specifics (whose key, which pool, what date),
/// never on a bare kind label (founder UX report 2026-10-21).
pub fn summarize(kind: &str, payload: &Value) -> String {
    let str_of = |key: &str| payload[key].as_str().unwrap_or("");
    match kind {
        "admin_demote" => format!("Demote admin {} to member", str_of("email")),
        "admin_key_expiry" => {
            let owner = str_of("owner_email");
            let key = str_of("key_prefix");
            match payload["expires_at"].as_i64() {
                Some(ms) => format!(
                    "Set expiry {} on {} (owner {})",
                    iso_date_utc(ms),
                    key,
                    owner
                ),
                None => format!("Clear expiry on {key} (owner {owner})"),
            }
        }
        "admin_key_revoke" => format!(
            "Revoke {} (owner {})",
            str_of("key_prefix"),
            str_of("owner_email")
        ),
        "admin_key_rotate" => format!(
            "Rotate {} (owner {})",
            str_of("key_prefix"),
            str_of("owner_email")
        ),
        "org_policy" => match str_of("policy_action") {
            "configure" => format!(
                "Update classifier policy: {} mode, {} dialect",
                str_of("mode"),
                str_of("dialect")
            ),
            "go_live" => "Take classifier auto-block live".into(),
            "remove" => "Remove the classifier policy".into(),
            other => format!("Org policy change ({other})"),
        },
        "pool_disable" => format!("Disable pool '{}'", str_of("name")),
        other => format!("Sensitive action ({other})"),
    }
}

/// Epoch-ms → "2026-10-24" (UTC). No chrono dependency pull: the approvals
/// card needs a date, not a timezone — the dashboard renders local times
/// where precision matters.
fn iso_date_utc(ms: i64) -> String {
    let days = ms / 86_400_000;
    // Civil-from-days (Howard Hinnant's algorithm — public domain):
    // converts a day count since 1970-01-01 to y/m/d without a library.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// The audit-trail view of a pending-action payload: the sealed
/// classifier envelope (ciphertext/dek_wrapped) is DROPPED — it already
/// lives in pending_actions for execution; the audit row only needs the
/// human-meaningful parameters (final-scan finding F1: the blob was
/// duplicated into audit_log, widening sealed-key exposure for zero
/// functional need).
pub fn audit_view(payload: &Value) -> Value {
    let mut view = payload.clone();
    if let Some(obj) = view.as_object_mut() {
        obj.remove("ciphertext");
        obj.remove("dek_wrapped");
    }
    view
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

/// What an approved action produced: `summary` goes into the audit row
/// (masked, safe); `reveal` carries REVEAL-ONCE material that must reach
/// exactly the approver's HTTP response and NOTHING else (a rotated
/// admin key's raw form — never the audit, never a log).
#[derive(Debug)]
pub struct Executed {
    pub summary: Value,
    pub reveal: Option<Value>,
}

/// Executes an approved pending action. Re-validates every guard against
/// CURRENT state (the world may have moved between request and approval):
/// the demote target must still be an admin and must not be the last one;
/// key/pool targets must still exist. Returns a summary for the executed
/// action's audit row.
/// Prefix bailed by `execute` when the classifier config moved under a
/// pending approval (A-028). The approve endpoint maps it to 409 Conflict
/// (the ruling's wording) instead of the generic 400 — an HTTP client can
/// distinguish "retry the request fresh" from "fix your payload".
pub const STALE_EPOCH_BAIL: &str = "STALE_EPOCH:";

/// bails with the typed stale-epoch marker (see [`STALE_EPOCH_BAIL`]).
macro_rules! bail_stale_epoch {
    ($expected:expr, $current:expr) => {
        anyhow::bail!(
            "{}classifier config changed since this approval was requested \
             (config epoch {} → {}) — re-request it",
            STALE_EPOCH_BAIL,
            $expected,
            $current
        )
    };
}

pub async fn execute(db: &Db, action: &db::PendingActionRow, now: i64) -> anyhow::Result<Executed> {
    let payload: Value = serde_json::from_str(&action.payload_json).unwrap_or(Value::Null);
    match SensitiveKind::parse(&action.kind) {
        Some(SensitiveKind::AdminDemote) => {
            let target_id = payload["user_id"].as_str().unwrap_or(&action.target_id);
            // Last-admin guard AT EXECUTION, atomically: the ≥2 count
            // rides inside the UPDATE, so two concurrent demotes can
            // never both win and leave the org admin-less (final-scan).
            let affected = db::demote_org_admin_guarded(db, &action.org_id, target_id).await?;
            if affected == 0 {
                anyhow::bail!(
                    "refused: target is no longer an admin, or is the last remaining admin"
                );
            }
            Ok(Executed {
                summary: json!({
                    "user_id": target_id,
                    "role_after": "member",
                }),
                reveal: None,
            })
        }
        Some(SensitiveKind::AdminKeyExpiry) => {
            let key_id = payload["key_id"].as_str().unwrap_or(&action.target_id);
            let expires_at = payload["expires_at"].as_i64();
            db::set_pool_key_expiry(db, key_id, expires_at).await?;
            Ok(Executed {
                summary: json!({ "key_id": key_id, "expires_at": expires_at }),
                reveal: None,
            })
        }
        Some(SensitiveKind::AdminKeyRevoke) => {
            let key_id = payload["key_id"].as_str().unwrap_or(&action.target_id);
            let affected = db::revoke_pool_key(db, key_id).await?;
            if affected == 0 {
                anyhow::bail!("refused: key already revoked or gone");
            }
            Ok(Executed {
                summary: json!({ "key_id": key_id, "status": "revoked" }),
                reveal: None,
            })
        }
        Some(SensitiveKind::AdminKeyRotate) => {
            // Deferred rotation: NOTHING was generated at request time;
            // execution mints the credential and reveals it ONCE — to
            // the approving admin only (requester proposed, distinct
            // admin approved AND received; audit carries the masked
            // summary alone).
            let key_id = payload["key_id"].as_str().unwrap_or(&action.target_id);
            let user_id = payload["user_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("rotate payload lost its owner"))?;
            let generated = tributary_common::keys::generate_key();
            let new_id = tributary_common::db::new_id();
            // Atomic (F1, round 2): mint-new + kill-old + allowlist-copy
            // share ONE transaction. The old code inserted the new key
            // BEFORE the guard, so a key that died between request and
            // approval left an orphan `active` row behind a rejected
            // action (and suspended keys hit that path deterministically).
            let rotated = db::rotate_key_atomic(
                db,
                key_id,
                &db::NewPoolKey {
                    id: new_id.clone(),
                    pool_id: payload["pool_id"].as_str().unwrap_or_default().to_string(),
                    key_hash: generated.key_hash,
                    key_prefix: generated.key_prefix.clone(),
                    user_id: user_id.to_string(),
                    expires_at: payload["expires_at"].as_i64(),
                    created_at: now,
                },
            )
            .await?;
            if !rotated {
                anyhow::bail!("refused: original key is no longer rotatable — nothing was minted");
            }
            Ok(Executed {
                summary: json!({
                    "key_id": new_id,
                    "replaces": key_id,
                    "key_prefix": generated.key_prefix,
                    "status": "active",
                }),
                reveal: Some(json!({
                    "key_id": new_id,
                    "raw": generated.raw,
                    "key_prefix": generated.key_prefix,
                })),
            })
        }
        Some(SensitiveKind::OrgPolicy) => match payload["policy_action"].as_str() {
            Some("configure") => {
                // A-028 config-epoch guard: the payload froze the config
                // the REQUESTER saw. If the row mutated since (another
                // approval executed, a reconfigure landed), executing
                // this would silently overwrite the newer world — refuse;
                // the requester re-requests against the present.
                let prior = db::classifier_config_by_org(db, &action.org_id).await?;
                let current_epoch = prior.as_ref().map(|p| p.config_epoch).unwrap_or(0);
                let expected_epoch = payload["config_epoch"].as_i64().unwrap_or(-1);
                if current_epoch != expected_epoch {
                    bail_stale_epoch!(expected_epoch, current_epoch);
                }
                // The classifier row was built (and its key SEALED) at
                // request time; execution only refreshes the timestamps —
                // entering auto_block starts its shadow window NOW.
                let mut row = classifier_row_from_payload(&payload);
                row.updated_at = now;
                // Same live-preservation rule as the direct PUT
                // (final-scan A3, narrowed round 2 / R2-1): live survives
                // only sampling/key tweaks. Vendor (base_url/dialect/
                // model), purpose statement, mode, or threshold changes
                // re-enter shadow — new enforcement semantics must be
                // re-validated before they can block.
                let (live, shadow) = match &prior {
                    Some(p)
                        if p.auto_block_live
                            && row.mode == "auto_block"
                            && p.alert_threshold_bp == row.alert_threshold_bp
                            && p.block_threshold_bp == row.block_threshold_bp
                            && p.base_url == row.base_url
                            && p.dialect == row.dialect
                            && p.model == row.model
                            && p.purpose_statement == row.purpose_statement =>
                    {
                        (true, p.shadow_started_at)
                    }
                    _ => (
                        false,
                        if row.mode == "auto_block" {
                            Some(now)
                        } else {
                            None
                        },
                    ),
                };
                row.auto_block_live = live;
                row.shadow_started_at = shadow;
                db::upsert_classifier_config(db, &row).await?;
                Ok(Executed {
                    summary: json!({
                        "policy": "classifier_configured",
                        "dialect": row.dialect,
                        "model": row.model,
                        "mode": row.mode,
                    }),
                    reveal: None,
                })
            }
            Some("remove") => {
                // A-028: same epoch guard — a remove requested against a
                // config that has since been reconfigured must not fire.
                let prior = db::classifier_config_by_org(db, &action.org_id).await?;
                let current_epoch = prior.as_ref().map(|p| p.config_epoch).unwrap_or(0);
                let expected_epoch = payload["config_epoch"].as_i64().unwrap_or(-1);
                if current_epoch != expected_epoch {
                    bail_stale_epoch!(expected_epoch, current_epoch);
                }
                db::delete_classifier_config(db, &action.org_id).await?;
                Ok(Executed {
                    summary: json!({ "policy": "classifier_removed" }),
                    reveal: None,
                })
            }
            Some("go_live") => {
                // A-028 epoch guard + optional minimum shadow age.
                let prior = db::classifier_config_by_org(db, &action.org_id).await?;
                let current_epoch = prior.as_ref().map(|p| p.config_epoch).unwrap_or(0);
                let expected_epoch = payload["config_epoch"].as_i64().unwrap_or(-1);
                if current_epoch != expected_epoch {
                    bail_stale_epoch!(expected_epoch, current_epoch);
                }
                enforce_min_shadow(db, &action.org_id, prior.as_ref(), now).await?;
                let affected = db::classifier_go_live(db, &action.org_id, now).await?;
                if affected == 0 {
                    anyhow::bail!("refused: no auto_block config to go live");
                }
                Ok(Executed {
                    summary: json!({ "policy": "classifier_go_live" }),
                    reveal: None,
                })
            }
            other => anyhow::bail!("unknown policy action: {other:?}"),
        },
        Some(SensitiveKind::PoolDisable) => {
            let pool_id = payload["pool_id"].as_str().unwrap_or(&action.target_id);
            db::disable_pool(db, pool_id).await?;
            Ok(Executed {
                summary: json!({ "pool_id": pool_id, "status": "disabled" }),
                reveal: None,
            })
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
        // A-028: the epoch the REQUESTER observed — carried through so the
        // execute-side guard can compare it with the present row.
        config_epoch: payload["config_epoch"].as_i64().unwrap_or(-1),
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
        "config_epoch": row.config_epoch,
    })
}

/// A-028 minimum-shadow-age gate, shared by the deferred execute path and
/// the direct (solo org) go-live: when the org opted into
/// `classifier_min_shadow_secs > 0`, a go-live may only execute once the
/// current shadow window is at least that old. Default 0 = dormant.
pub async fn enforce_min_shadow(
    db: &Db,
    org_id: &str,
    prior: Option<&db::ClassifierConfigRow>,
    now: i64,
) -> anyhow::Result<()> {
    let settings = db::org_settings(db, org_id).await?;
    if settings.classifier_min_shadow_secs <= 0 {
        return Ok(());
    }
    let Some(row) = prior else {
        anyhow::bail!("refused: no classifier config to go live");
    };
    let Some(started) = row.shadow_started_at else {
        anyhow::bail!("refused: config has no shadow window (mode is alert-only)");
    };
    let age_secs = (now - started).max(0) / 1000;
    if age_secs < settings.classifier_min_shadow_secs {
        anyhow::bail!(
            "refused: shadow window is {age_secs}s old; org policy requires at least \
             {}s before go-live (classifier_min_shadow_secs)",
            settings.classifier_min_shadow_secs
        );
    }
    Ok(())
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
            SensitiveKind::AdminKeyRotate,
            SensitiveKind::OrgPolicy,
            SensitiveKind::PoolDisable,
        ] {
            assert_eq!(SensitiveKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(SensitiveKind::parse("nope"), None);
    }

    #[test]
    fn summaries_name_the_target_not_the_kind() {
        // The founder's UX bar: an approver reads WHO/WHAT, never a bare
        // kind label (report 2026-10-21).
        let revoke = json!({"key_prefix": "sk-pool-6b67e…73e4", "owner_email": "b@t.dev"});
        assert_eq!(
            summarize("admin_key_revoke", &revoke),
            "Revoke sk-pool-6b67e…73e4 (owner b@t.dev)"
        );
        let expiry = json!({
            "key_prefix": "sk-pool-6b67e…73e4",
            "owner_email": "b@t.dev",
            // 2026-10-24T00:00:00Z in epoch ms.
            "expires_at": 1_792_800_000_000i64,
        });
        let summary = summarize("admin_key_expiry", &expiry);
        assert!(
            summary.starts_with("Set expiry 2026-10-24 on sk-pool-6b67e…73e4 (owner b@t.dev)"),
            "got: {summary}"
        );
        let demote = json!({"email": "second@t.dev"});
        assert_eq!(
            summarize("admin_demote", &demote),
            "Demote admin second@t.dev to member"
        );
        let pool = json!({"name": "eyes-pool"});
        assert_eq!(summarize("pool_disable", &pool), "Disable pool 'eyes-pool'");
    }

    #[test]
    fn iso_date_known_values() {
        assert_eq!(iso_date_utc(0), "1970-01-01");
        // Leap day: 2024-02-29T00:00:00Z = 1_709_164_800_000 ms.
        assert_eq!(iso_date_utc(1_709_164_800_000), "2024-02-29");
        assert_eq!(iso_date_utc(1_792_800_000_000), "2026-10-24");
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
            config_epoch: 7,
        };
        let payload = classifier_payload(&row, "configure");
        let back = classifier_row_from_payload(&payload);
        assert_eq!(back.org_id, row.org_id);
        assert_eq!(back.dialect, "jev");
        assert_eq!(back.ciphertext, "ct");
        assert_eq!(back.mode, "auto_block");
        // A-028: the observed epoch rides the payload round-trip.
        assert_eq!(back.config_epoch, 7);
        // Never carried: execution-time fields reset by design.
        assert_eq!(back.updated_at, 0);
        assert!(back.shadow_started_at.is_none());
    }
}
