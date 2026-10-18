//! Email alerts for leak detection (SPEC §9 free tier, M4).
//!
//! Design constraints from the kickoff: SMTP settings come from
//! `TRIBUTARY_SMTP_*` env only; bodies are PLAIN TEXT built with `format!`
//! (no templating libs); sending is ASYNC and off the request path; when
//! SMTP is unconfigured the alert falls back to a loud log line while the
//! outbox row persists (so tests and ops can still prove the alert fired).
//!
//! The outbox (`email_alerts` table) is the source of truth for "an alert
//! happened"; `status` records the delivery attempt outcome.

use lettre::AsyncSmtpTransport;
use lettre::AsyncTransport;
use lettre::Message;
use lettre::Tokio1Executor;
use lettre::transport::smtp::authentication::Credentials;
use tributary_common::db::{Db, now_ms};

use crate::db;

/// SMTP relay settings parsed once at startup from the environment.
///
/// `None` (the whole struct absent) = feature unconfigured = every alert
/// takes the log-fallback path.
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

impl SmtpConfig {
    /// Reads `TRIBUTARY_SMTP_{HOST,PORT,USERNAME,PASSWORD,FROM}`. All five
    /// are required together — a half-configured relay would silently eat
    /// alerts, so partial config is treated as unconfigured (with a warn).
    pub fn from_env() -> Option<Self> {
        let get = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        match (
            get("TRIBUTARY_SMTP_HOST"),
            get("TRIBUTARY_SMTP_PORT"),
            get("TRIBUTARY_SMTP_USERNAME"),
            get("TRIBUTARY_SMTP_PASSWORD"),
            get("TRIBUTARY_SMTP_FROM"),
        ) {
            (Some(host), port, Some(username), Some(password), Some(from)) => {
                // Default port 587 (submission + STARTTLS) unless overridden.
                let port = port.and_then(|p| p.parse::<u16>().ok()).unwrap_or(587);
                Some(Self {
                    host,
                    port,
                    username,
                    password,
                    from,
                })
            }
            _ => {
                tracing::warn!(
                    "TRIBUTARY_SMTP_* not fully set — email alerts will use the log fallback"
                );
                None
            }
        }
    }
}

/// Queues one alert email row (status 'queued') and returns its id. The
/// caller decides whether/when to attempt delivery via [`try_send`] —
/// queueing alone never fails a request.
pub async fn queue(
    db: &Db,
    org_id: &str,
    to_email: &str,
    subject: &str,
    body: &str,
) -> sqlx::Result<String> {
    let id = tributary_common::db::new_id();
    db::insert_email_alert(db, &id, org_id, to_email, subject, body, now_ms()).await?;
    Ok(id)
}

/// Attempts delivery of one queued row, then records the outcome.
///
/// - SMTP configured → send; `sent` on success, `skipped` + error log on
///   failure (fail-open: an alerting outage must not break the product).
/// - SMTP unconfigured → `skipped` + a loud warn — the log-fallback path.
pub async fn try_send(
    db: &Db,
    smtp: Option<&SmtpConfig>,
    alert_id: &str,
    to: &str,
    subject: &str,
    body: &str,
) {
    let Some(smtp) = smtp else {
        // Log fallback: the row stays as durable proof; ops sees the warn.
        tracing::warn!("email alert {alert_id} to {to}: SMTP unconfigured — logged, not sent");
        let _ = db::set_email_alert_status(db, alert_id, "skipped", None).await;
        return;
    };
    match send_via_relay(smtp, to, subject, body).await {
        Ok(()) => {
            let _ = db::set_email_alert_status(db, alert_id, "sent", Some(now_ms())).await;
        }
        Err(err) => {
            // Bodies are plain text built from masked data — but we still
            // log only the error, never the body itself.
            tracing::error!("email alert {alert_id} send failed: {err:#}");
            let _ = db::set_email_alert_status(db, alert_id, "skipped", None).await;
        }
    }
}

/// Sends one plain-text email through the configured relay (STARTTLS).
///
/// `lettre`'s `starttls_relay` requires a TLS feature — we build with
/// `tokio1-rustls-tls` (no native-tls, keeping the OpenSSL-free build D30
/// established for cross-compiles).
async fn send_via_relay(
    smtp: &SmtpConfig,
    to: &str,
    subject: &str,
    body: &str,
) -> anyhow::Result<()> {
    let email = Message::builder()
        .from(smtp.from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .body(body.to_string())?;
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)?
        .port(smtp.port)
        .credentials(Credentials::new(
            smtp.username.clone(),
            smtp.password.clone(),
        ))
        .build();
    mailer.send(email).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_config_from_env_tolerates_empty_environment() {
        // In the test environment nothing is configured → None, no panic.
        // (Guarded so a developer's local TRIBUTARY_SMTP_* doesn't flake it.)
        if std::env::var("TRIBUTARY_SMTP_HOST").is_err() {
            assert!(SmtpConfig::from_env().is_none());
        }
    }
}
