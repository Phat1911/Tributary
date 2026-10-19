//! Detection pipeline for the M4 leak-prevention free tier (SPEC §9, D17).
//!
//! One entry point — [`detect`] — fans every free-tier detection out to the
//! three surfaces the spec requires:
//!
//! 1. a **review flag** in the admin queue (detection is free; the *response*
//!    is a human decision — tarpit/auto-revoke stay paid, D17),
//! 2. an **audit row** (actor `system`, sanitized detail),
//! 3. an optional **email alert** (async outbox send, log-fallback).
//!
//! The gateway data plane calls this on classifier flags/auto-blocks and
//! impossible travel (canary sensors were removed post-M4, D31). Writes are
//! best-effort-logged (a failing detection write must not change the
//! client-visible response), but on the
//! happy path all rows land before the caller proceeds, so tests can assert
//! them deterministically.

use serde_json::Value;
use tributary_common::db::{Db, new_id, now_ms};

use crate::audit::{self, AuditEntry, TargetKind};
use crate::db;
use crate::notify::{self, SmtpConfig};

/// Which free-tier sensor fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionKind {
    /// The purpose classifier scored a request below the alert threshold
    /// (D28). The member's key stays live unless auto-block is LIVE.
    ClassifierFlag,
    /// Auto-block (live mode) suspended a key pending review (D28: suspends
    /// the KEY only — never bans the person; one-click unblock).
    ClassifierAutoBlock,
    /// The same key appeared from two different countries faster than
    /// travel allows (dormant unless a GeoLite2 db is configured).
    ImpossibleTravel,
}

impl DetectionKind {
    /// Wire form stored in `review_flags.kind`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClassifierFlag => "classifier_flag",
            Self::ClassifierAutoBlock => "classifier_auto_block",
            Self::ImpossibleTravel => "impossible_travel",
        }
    }

    /// The audit-log action for this sensor.
    fn audit_action(&self) -> &'static str {
        match self {
            Self::ClassifierFlag => "security.classifier_flag",
            Self::ClassifierAutoBlock => "security.classifier_auto_block",
            Self::ImpossibleTravel => "security.impossible_travel",
        }
    }

    /// Default severity in the review queue.
    fn default_severity(&self) -> &'static str {
        match self {
            Self::ClassifierFlag => "medium",
            Self::ClassifierAutoBlock => "high",
            Self::ImpossibleTravel => "medium",
        }
    }

    /// One-line human label for email subjects/bodies.
    fn label(&self) -> &'static str {
        match self {
            Self::ClassifierFlag => "off-purpose request flagged",
            Self::ClassifierAutoBlock => "off-purpose request auto-blocked (key suspended)",
            Self::ImpossibleTravel => "impossible travel between countries",
        }
    }
}

/// Severity override for the queue row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl Severity {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// One detection event, as handed to [`detect`] by the data plane.
///
/// `detail` is ALREADY sanitized by the caller (masked key prefix, model
/// name, IPs, score) — this module does not have a path that accepts
/// prompts or key material, mirroring the audit-log invariant.
#[derive(Debug, Clone)]
pub struct Detection<'a> {
    pub org_id: &'a str,
    pub pool_id: Option<&'a str>,
    pub pool_key_id: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub kind: DetectionKind,
    pub severity: Option<Severity>,
    /// Classifier alignment in basis points (0–10000), when scored.
    pub score_bp: Option<i64>,
    /// Sanitized context JSON for the reviewer.
    pub detail: Value,
    /// Queue an email alert too? (Classifier sensors yes; noisy
    /// low-value sensors can opt out.)
    pub email: bool,
}

/// Reserved audit identity for pipeline-written rows (the `audit_log`
/// migration reserves `'system'` for internals).
const SYSTEM_ACTOR_ID: &str = "system";
const SYSTEM_ACTOR_EMAIL: &str = "system@tributary.internal";

/// Fires the full detection pipeline. Returns the created flag id (or
/// `None` when even the queue write failed — logged loudly).
///
/// Best-effort by design: each write logs its own failure and the pipeline
/// continues, because the caller's response shape (e.g. the honey key's
/// generic 401) must not depend on detection plumbing.
pub async fn detect(db: &Db, smtp: Option<&SmtpConfig>, event: &Detection<'_>) -> Option<String> {
    let flag_id = new_id();
    let ts = now_ms();
    let severity = event
        .severity
        .map(|s| s.as_str())
        .unwrap_or_else(|| event.kind.default_severity());

    // 1) Review queue — the admin's one place to look.
    if let Err(err) = db::insert_review_flag(
        db,
        &db::NewReviewFlag {
            id: &flag_id,
            org_id: event.org_id,
            pool_id: event.pool_id,
            pool_key_id: event.pool_key_id,
            user_id: event.user_id,
            kind: event.kind.as_str(),
            severity,
            score_bp: event.score_bp,
            detail_json: &event.detail,
            created_at: ts,
        },
    )
    .await
    {
        tracing::error!(
            "review flag insert failed (kind={} org={}): {err:#}",
            event.kind.as_str(),
            event.org_id
        );
        return None;
    }

    // 2) Audit row — the immutable trail (sanitized detail only).
    audit::record(
        db,
        AuditEntry {
            org_id: Some(event.org_id),
            actor_id: SYSTEM_ACTOR_ID,
            actor_email: SYSTEM_ACTOR_EMAIL,
            action: event.kind.audit_action(),
            target_kind: TargetKind::ReviewFlag,
            target_id: &flag_id,
            before: None,
            after: Some(event.detail.clone()),
            // The data plane's view of the client IP lives in the detail;
            // the control-plane `ip` column is left NULL for system rows.
            ip: None,
        },
    )
    .await;

    // 3) Email alert (async send; outbox row persists even on fallback).
    // Fan out one outbox row per recipient, then hand delivery to spawned
    // tasks — the request path never waits on SMTP (or its absence).
    // Lookup/insert failures only LOG: they never taint the already
    // persisted flag + audit rows above.
    if event.email {
        let recipients = db::alert_recipients(db, event.org_id)
            .await
            .unwrap_or_else(|err| {
                tracing::error!(
                    "alert recipient lookup failed for org {}: {err:#}",
                    event.org_id
                );
                Vec::new()
            });
        if recipients.is_empty() {
            tracing::warn!(
                "detection {} queued but no alert recipient exists for org {}",
                event.kind.as_str(),
                event.org_id
            );
        }
        // Build the (already-sanitized) text BEFORE spawning: `event`
        // borrows from the caller and cannot cross the 'static task
        // boundary. Each task OWNS its cloned copies (the borrowed
        // alternative would dangle once this block returns).
        let subject_text = subject(event);
        let body_text = body(event);
        for to in recipients {
            let Ok(alert_id) =
                notify::queue(db, event.org_id, &to, &subject_text, &body_text).await
            else {
                tracing::error!("email alert outbox insert failed for org {}", event.org_id);
                continue;
            };
            let db_clone = db.clone();
            let smtp_clone = smtp.cloned();
            let subject_owned = subject_text.clone();
            let body_owned = body_text.clone();
            tokio::spawn(async move {
                notify::try_send(
                    &db_clone,
                    smtp_clone.as_ref(),
                    &alert_id,
                    &to,
                    &subject_owned,
                    &body_owned,
                )
                .await;
            });
        }
    }

    Some(flag_id)
}

// The email text builders are plain `format!` on the SANITIZED detail —
// no templating library (kickoff constraint), no interpolation of prompts.

/// Email subject for a detection.
fn subject(event: &Detection<'_>) -> String {
    format!(
        "[Tributary] {}: {}",
        event.kind.label(),
        event
            .detail
            .get("key_prefix")
            .and_then(Value::as_str)
            .unwrap_or("key")
    )
}

/// Plain-text email body for a detection.
fn body(event: &Detection<'_>) -> String {
    let score_line = match event.score_bp {
        Some(bp) => format!("\nAlignment score: {:.2}", bp as f64 / 10_000.0),
        None => String::new(),
    };
    format!(
        "A Tributary leak-prevention detection fired.\n\n\
         Kind: {}\n\
         Detection id: see the review queue\n\
         Key: {}{}\n\
         Detail: {}\n\n\
         Open the dashboard review queue to act (unblock, dismiss, or revoke).\n\
         This is an automated message; responses are not monitored.",
        event.kind.label(),
        event
            .detail
            .get("key_prefix")
            .and_then(Value::as_str)
            .unwrap_or("n/a"),
        score_line,
        event.detail,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_map_to_stable_wire_forms() {
        assert_eq!(DetectionKind::ClassifierFlag.as_str(), "classifier_flag");
        assert_eq!(
            DetectionKind::ClassifierAutoBlock.as_str(),
            "classifier_auto_block"
        );
        assert_eq!(
            DetectionKind::ImpossibleTravel.as_str(),
            "impossible_travel"
        );
        assert_eq!(DetectionKind::ClassifierFlag.default_severity(), "medium");
    }

    #[test]
    fn email_body_is_plain_text_and_sanitized() {
        let event = Detection {
            org_id: "org-1",
            pool_id: Some("pool-1"),
            pool_key_id: Some("key-1"),
            user_id: None,
            kind: DetectionKind::ClassifierFlag,
            severity: None,
            score_bp: Some(420),
            detail: json!({ "key_prefix": "sk-pool-abcd…wxyz", "score": 0.42 }),
            email: false,
        };
        let text = body(&event);
        assert!(text.contains("sk-pool-abcd…wxyz"));
        assert!(text.contains("off-purpose"));
        assert!(text.contains("review queue"));
        assert!(!text.contains("<") || !text.contains("<html>"));
    }
}
