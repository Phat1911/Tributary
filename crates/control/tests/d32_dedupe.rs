//! A-021 (M5 hardening): D32 flag de-duplication is a DATABASE invariant.
//!
//! Red-first regression: before the fix, `security::detect` de-duplicated by
//! count-then-insert — a concurrent burst from a newly shared key raised N
//! flags + N emails for one sharing event. This file pins the sequential
//! half deterministically (two detections, same key, same window ⇒ exactly
//! one flag/email); the CONCURRENT half is the gateway journey
//! `m5_journeys::j3` (public API, workflow step 12).
//!
//! It also pins the SUPERSEDE rule: an OPEN d32 flag older than the org's
//! window belongs to a PREVIOUS incident — a new sharing event must raise a
//! NEW flag, not be swallowed forever by the unique index.

mod harness;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::Row;
use tributary_common::db::Db;
use tributary_control::security::{Detection, DetectionKind};

/// Provisions org + one org_admin (the alert recipient) through the public
/// API, then returns (org_id, admin_token). The pool key id is fabricated —
/// `review_flags` carries no FK and the product's own provisioning path for
/// keys is irrelevant to the dedupe invariant under test.
async fn org_with_recipient(db: &Db, app: &axum::Router) -> (String, String) {
    let host = harness::bootstrap_and_login(db, app).await;
    let (status, body) = harness::send(
        app,
        harness::post_json(
            "/api/v1/orgs",
            Some(&host),
            &json!({"name": "Dedupe Org", "slug": harness::unique_slug("dedupe")}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "org created: {body}");
    let org_id = body["id"].as_str().unwrap().to_string();
    // One org_admin = exactly one alert recipient (deterministic email count).
    let (status, body) = harness::send(
        app,
        harness::post_json(
            "/api/v1/members",
            Some(&host),
            &json!({
                "org_id": org_id,
                "email": harness::unique_email("admin"),
                "password": "hunter2hunter2",
                "role": "org_admin",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "org admin created: {body}");
    (org_id, host)
}

/// One D32 detection, exactly as the gateway chat path raises it.
async fn d32_detect(db: &Db, org_id: &str, key_id: &str) -> Option<String> {
    tributary_control::security::detect(
        db,
        None,
        &Detection {
            org_id,
            pool_id: Some("pool-irrelevant"),
            pool_key_id: Some(key_id),
            user_id: None,
            kind: DetectionKind::DistinctIpConcurrency,
            severity: None,
            score_bp: None,
            detail: json!({
                "key_prefix": "sk-pool-test…0000",
                "ip": "203.0.113.7",
                "shared_networks": ["91.207.174.0", "203.0.113.0"],
                "window_secs": 300,
            }),
            email: true,
        },
    )
    .await
}

/// Open d32 flags for one key (the queue the admin actually sees).
async fn open_d32_count(db: &Db, key_id: &str) -> i64 {
    let sql = db.sql(
        "SELECT COUNT(*) AS n FROM review_flags \
         WHERE pool_key_id = ? AND kind = 'distinct_ip_concurrency' AND status = 'open'",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(key_id)
        .fetch_one(db.pool())
        .await
        .expect("flag count");
    row.get::<i64, _>("n")
}

/// Outbox rows for the org (one per queued alert email).
async fn outbox_count(db: &Db, org_id: &str) -> i64 {
    let sql = db.sql("SELECT COUNT(*) AS n FROM email_alerts WHERE org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .fetch_one(db.pool())
        .await
        .expect("outbox count");
    row.get::<i64, _>("n")
}

#[tokio::test]
async fn d32_one_open_flag_and_email_per_incident() {
    for test_db in harness::test_dbs().await {
        let (app, _mock) = harness::control_app(test_db.pool.clone()).await;
        let (org_id, _host) = org_with_recipient(&test_db.pool, &app).await;
        let key_id = format!("key-{}", harness::unique_suffix());

        // The same sharing event, detected twice inside the window (two
        // racing requests, sequentially interleaved): exactly ONE flag
        // may exist, and the second detection must be a no-op.
        let first = d32_detect(&test_db.pool, &org_id, &key_id).await;
        assert!(first.is_some(), "the first detection raises the flag");
        let second = d32_detect(&test_db.pool, &org_id, &key_id).await;
        assert_eq!(
            open_d32_count(&test_db.pool, &key_id).await,
            1,
            "duplicate detection adds no second open flag [{}]",
            test_db.name
        );
        // Pre-fix this is 2: the count-then-insert race raised one email
        // per detection. Post-fix the deduped insert short-circuits before
        // the outbox write.
        assert_eq!(
            outbox_count(&test_db.pool, &org_id).await,
            1,
            "one sharing event queues exactly one alert email [{}]",
            test_db.name
        );
        // `second` is the pipeline's own claim about what it did.
        assert!(second.is_none(), "the duplicate detection is a no-op");
    }
}

#[tokio::test]
async fn d32_stale_open_flag_is_superseded_not_swallowed() {
    for test_db in harness::test_dbs().await {
        let (app, _mock) = harness::control_app(test_db.pool.clone()).await;
        let (org_id, _host) = org_with_recipient(&test_db.pool, &app).await;
        let key_id = format!("key-{}", harness::unique_suffix());

        let first_id = d32_detect(&test_db.pool, &org_id, &key_id)
            .await
            .expect("first incident flags");
        // Backdate the open flag past the window: it now belongs to a
        // PREVIOUS incident (test-only time travel — the product has no
        // API for aging flags; direct row UPDATE is the only way).
        let sql = test_db
            .pool
            .sql("UPDATE review_flags SET created_at = ? WHERE id = ?");
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(tributary_common::db::now_ms() - 600_000) // 10 min ago > 300 s window
            .bind(&first_id)
            .execute(test_db.pool.pool())
            .await
            .expect("backdate");

        // A NEW sharing event (same key, new window): it must raise a NEW
        // flag — the unique index on open d32 flags must not silence the
        // second incident; the stale flag is superseded (resolved), not
        // left dangling.
        let second_id = d32_detect(&test_db.pool, &org_id, &key_id).await;
        assert!(
            second_id.is_some(),
            "a new incident raises a new flag [{}]",
            test_db.name
        );
        assert_eq!(
            open_d32_count(&test_db.pool, &key_id).await,
            1,
            "exactly one OPEN flag after supersede [{}]",
            test_db.name
        );
        // The old flag must no longer be open — the queue shows the
        // current incident only.
        let sql = test_db
            .pool
            .sql("SELECT status FROM review_flags WHERE id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&first_id)
            .fetch_one(test_db.pool.pool())
            .await
            .expect("old flag row");
        assert_ne!(
            row.get::<String, _>("status"),
            "open",
            "the stale flag was superseded [{}]",
            test_db.name
        );
    }
}
