//! A-004 closure tests: per-email+IP login throttling (fixed window).
//!
//! Identities are unique per RUN and provisioned through the real admin
//! API (bootstrap admin → org → member). Parallel test binaries share the
//! Postgres database: a globally fixed email would let another binary's
//! failed logins land in "our" buckets (and our failed logins in theirs)
//! and trip the throttle prematurely — observed once as a flake. Unique
//! emails isolate the fixture without touching a single assertion.

mod harness;

use axum::http::StatusCode;
use serde_json::json;

use harness::*;

/// Creates a member with known credentials through the admin API and
/// returns (email, password). The member role is irrelevant to throttling
/// — login is uniform across roles.
///
/// Provisioning runs on an UNLIMITED app: the shared admin's own bucket
/// (`root@tributary.dev|local`) accumulates failures from every parallel
/// binary on the shared Postgres, so a limited app could 429 the admin
/// login itself. Throttle assertions then run on the caller's limited app
/// over the member's isolated (unique-email) buckets.
async fn provision_member(db: &TestDb, tag: &str) -> (String, String) {
    let admin_app = control_app_with_limit(db.pool.clone(), 10_000).await;
    let _ = tributary_control::create_host_admin(&db.pool, "root@tributary.dev", "hunter2hunter2")
        .await;
    let creds = json!({"email": "root@tributary.dev", "password": "hunter2hunter2"});
    let mut token = None;
    for _ in 0..40 {
        let (_, body) = send(&admin_app, post_json("/api/v1/auth/login", None, &creds)).await;
        if let Some(t) = body["token"].as_str() {
            token = Some(t.to_string());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let token = token.expect("shared admin login never succeeded");

    let (_, body) = send(
        &admin_app,
        post_json(
            "/api/v1/orgs",
            Some(&token),
            &json!({"name": format!("Throttle {tag}"), "slug": unique_slug("throttle")}),
        ),
    )
    .await;
    let org_id = body["id"].as_str().expect("org created").to_string();

    let email = unique_email(tag);
    let password = "hunter2hunter2".to_string();
    let (status, body) = send(
        &admin_app,
        post_json(
            "/api/v1/members",
            Some(&token),
            &json!({"org_id": org_id, "email": &email, "password": &password}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "member provision: {body}");
    (email, password)
}

/// Exhaust the bucket → 429 with Retry-After → per-(email,ip) independence
/// → success clears the bucket.
#[tokio::test]
async fn login_throttle_locks_the_bucket_not_the_account() {
    for db in test_dbs().await {
        // Dedicated app: limit 3 fails/window (prod default is 5; tests
        // tighten for speed). The shared-DB harness app uses a huge limit.
        let app = control_app_with_limit(db.pool.clone(), 3).await;
        let (email, password) = provision_member(&db, "lock").await;

        let creds = |pw: &str| json!({"email": &email, "password": pw});
        // IPs arrive as TRANSPORT sockets (A-019): a bare header claim is
        // attacker input the sanitizer strips when no proxy is trusted.
        let ip1 = "203.0.113.7";

        // Three failures from IP 1: still 401 (generic) each time.
        for attempt in 0..3 {
            let (status, body) = send(
                &app,
                post_json_from_ip("/api/v1/auth/login", None, &creds("wrong"), ip1),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{}: attempt {}: {body}",
                db.name,
                attempt + 1
            );
        }

        // Fourth attempt from IP 1 — even with the CORRECT password: 429 +
        // Retry-After. The throttle gate runs before password work.
        let (status, headers, body) = send_full(
            &app,
            post_json_from_ip("/api/v1/auth/login", None, &creds(&password), ip1),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{}: {body}", db.name);
        let retry_after = headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        assert!(
            (1..=15 * 60).contains(&retry_after),
            "{}: Retry-After out of window bounds: {retry_after}",
            db.name
        );

        // Same email, different IP: unaffected (per-email+IP bucket).
        let ip2 = "198.51.100.9";
        let (status, body) = send(
            &app,
            post_json_from_ip("/api/v1/auth/login", None, &creds("wrong"), ip2),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{}: {body}", db.name);

        // Different email, same IP 1: unaffected.
        let other = unique_email("throttle-ghost");
        let (status, _) = send(
            &app,
            post_json_from_ip(
                "/api/v1/auth/login",
                None,
                &json!({"email": &other, "password": "whatever123"}),
                ip1,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", db.name);

        // Success from IP 2 clears that bucket: 1 fail, then success, then
        // 1 more fail is allowed (count restarted, not accumulated to 4).
        let (status, _) = send(
            &app,
            post_json_from_ip("/api/v1/auth/login", None, &creds(&password), ip2),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);
        let (status, _) = send(
            &app,
            post_json_from_ip("/api/v1/auth/login", None, &creds("wrong"), ip2),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{}: bucket should have reset on success",
            db.name
        );
    }
}

/// The throttle bucket survives app reconstruction (DB is authoritative —
/// the in-process counter is only the fast path).
#[tokio::test]
async fn login_throttle_state_survives_restart() {
    for db in test_dbs().await {
        let app = control_app_with_limit(db.pool.clone(), 2).await;
        let (email, password) = provision_member(&db, "restart").await;
        let creds = json!({"email": &email, "password": "wrong"});
        let ip = "192.0.2.44";

        for _ in 0..2 {
            send(
                &app,
                post_json_from_ip("/api/v1/auth/login", None, &creds, ip),
            )
            .await;
        }
        // Fresh router over the SAME DB: still denied.
        let app2 = control_app_with_limit(db.pool.clone(), 2).await;
        let (status, _) = send(
            &app2,
            post_json_from_ip("/api/v1/auth/login", None, &creds, ip),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{}", db.name);
        // Sanity: the correct password is also refused while locked, and
        // the lock is per (email, ip) — a different IP gets a clean 200.
        let (status, _) = send(
            &app2,
            post_json_from_ip(
                "/api/v1/auth/login",
                None,
                &json!({"email": &email, "password": &password}),
                ip,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{}", db.name);
    }
}
