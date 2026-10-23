//! J5 (M5): A-028 — a stale classifier approval arms nothing.
//!
//! Two journeys, public API only (workflow step 12):
//! 1. **Config-epoch binding**: in a dual-admin org, two configures
//!    requested against the same observed epoch; the approver approves in
//!    an order that invalidates the loser. The loser must 409, finalize
//!    `rejected`, and leave the winner's config untouched ("arms
//!    nothing"); a FRESH request against the present epoch recovers.
//! 2. **Minimum shadow age** (optional org setting, default OFF): with
//!    `classifier_min_shadow_secs` set, go-live refuses until the shadow
//!    window is that old — on the direct (solo) path too. Setting 0
//!    returns to dormant.
//!
//! Both ran RED against pre-A-028 code (last-writer-wins approvals; the
//! go-live flipped immediately) — evidence target/red-proof/.

mod harness;

use axum::http::StatusCode;
use serde_json::{Value, json};

use harness::*;

/// A real TCP scorer speaking the "custom" dialect (`POST /v1/align`) —
/// the save-time probe is an OUTBOUND reqwest call, so an in-process
/// Router is not enough; bind an ephemeral port and serve it.
async fn spawn_align_mock() -> String {
    let app = axum::Router::new().route(
        "/v1/align",
        axum::routing::post(|| async { axum::Json(json!({ "alignment": 0.9 })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind align mock");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

/// A valid auto_block configure body aimed at the mock.
fn classifier_body(base_url: &str, purpose: &str) -> Value {
    json!({
        "key": "align-mock-key-0123456789",
        "base_url": base_url,
        "dialect": "custom",
        "model": "align-mock",
        "purpose_statement": purpose,
        "mode": "auto_block",
        "acknowledge_auto_block": true,
    })
}

/// Org + two org admins + both tokens. The host bootstraps; the second
/// admin is provisioned through the members API (public provisioning
/// path, workflow step 12).
async fn dual_admin_org(app: &axum::Router, token: &str) -> (String, String, String) {
    let (_, body) = send(
        app,
        post_json(
            "/api/v1/orgs",
            Some(token),
            &json!({"name": "Epoch Org", "slug": unique_slug("epoch")}),
        ),
    )
    .await;
    let org_id = body["id"].as_str().expect("org created").to_string();

    // TWO org admins: the D33 gate counts org_admin users IN the org (the
    // host is not one), so deferral needs both members.
    let mut tokens = Vec::new();
    for n in 1..=2 {
        let email = unique_email("epoch-admin");
        let (status, body) = send(
            app,
            post_json(
                "/api/v1/members",
                Some(token),
                &json!({"org_id": org_id, "email": email, "password": "hunter2hunter2",
                        "role": "org_admin"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "org admin #{n}: {body}");
        let (_, body) = send(
            app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": email, "password": "hunter2hunter2"}),
            ),
        )
        .await;
        tokens.push(body["token"].as_str().expect("org admin login").to_string());
    }
    let mut it = tokens.into_iter();
    let token2 = it.next().expect("first org admin");
    let token3 = it.next().expect("second org admin");
    (org_id, token2, token3)
}

/// The pending org_policy action id from a deferred PUT (202 + row).
fn pending_id(body: &Value) -> String {
    body["id"]
        .as_str()
        .expect("deferred response carries the pending action")
        .to_string()
}

/// J5-A: stale approvals 409, arm nothing, and the world recovers.
#[tokio::test]
async fn j5_stale_classifier_approval_arms_nothing() {
    for db in test_dbs().await {
        let (app, _mock_upstream) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (org_id, token2, _token3) = dual_admin_org(&app, &token).await;
        let align = spawn_align_mock().await;

        // First configure (observed epoch 0 — no row yet) defers.
        let (status, p1) = send(
            &app,
            put_classifier(&org_id, &token, &classifier_body(&align, "purpose v1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{}: {p1}", db.name);

        // Approve #1: the row appears at epoch 1.
        let (status, body) = send(
            &app,
            post_json(
                &format!(
                    "/api/v1/orgs/{org_id}/pending-actions/{}/approve",
                    pending_id(&p1)
                ),
                Some(&token2),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}: approve #1: {body}", db.name);

        // Two configures against the SAME observed epoch (1).
        let (_, p2) = send(
            &app,
            put_classifier(&org_id, &token, &classifier_body(&align, "purpose v2")),
        )
        .await;
        let (_, p3) = send(
            &app,
            put_classifier(&org_id, &token, &classifier_body(&align, "purpose v3")),
        )
        .await;
        assert_eq!(p2["status"], "pending", "{}", db.name);
        assert_eq!(p3["status"], "pending", "{}", db.name);

        // Approve p2 first: epoch still 1 ⇒ executes ⇒ row now epoch 2
        // with v2's purpose.
        let (status, body) = send(
            &app,
            post_json(
                &format!(
                    "/api/v1/orgs/{org_id}/pending-actions/{}/approve",
                    pending_id(&p2)
                ),
                Some(&token2),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}: approve #2: {body}", db.name);

        // Approve p3 now: expected epoch 1 ≠ current 2 ⇒ 409, finalized
        // rejected, and the ROW keeps v2 (the stale config armed nothing).
        let (status, body) = send(
            &app,
            post_json(
                &format!(
                    "/api/v1/orgs/{org_id}/pending-actions/{}/approve",
                    pending_id(&p3)
                ),
                Some(&token2),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{}: stale approval must 409: {body}",
            db.name
        );
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("epoch"),
            "{}: 409 names the epoch conflict: {body}",
            db.name
        );
        let (_, p3_after) = send(
            &app,
            get_json(
                &format!("/api/v1/orgs/{org_id}/pending-actions?status=rejected"),
                Some(&token2),
            ),
        )
        .await;
        assert!(
            p3_after
                .as_array()
                .is_some_and(|rows| rows.iter().any(|r| r["id"] == pending_id(&p3))),
            "{}: stale action finalized rejected: {p3_after}",
            db.name
        );
        let (_, view) = send(
            &app,
            get_json(&format!("/api/v1/orgs/{org_id}/classifier"), Some(&token)),
        )
        .await;
        assert_eq!(
            view["purpose_statement"], "purpose v2",
            "{}: stale approval armed nothing: {view}",
            db.name
        );

        // Recovery: a FRESH request observes epoch 2 and executes cleanly.
        let (_, p4) = send(
            &app,
            put_classifier(&org_id, &token, &classifier_body(&align, "purpose v4")),
        )
        .await;
        let (status, body) = send(
            &app,
            post_json(
                &format!(
                    "/api/v1/orgs/{org_id}/pending-actions/{}/approve",
                    pending_id(&p4)
                ),
                Some(&token2),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}: fresh approve: {body}", db.name);
        let (_, view) = send(
            &app,
            get_json(&format!("/api/v1/orgs/{org_id}/classifier"), Some(&token)),
        )
        .await;
        assert_eq!(view["purpose_statement"], "purpose v4", "{}", db.name);
    }
}

/// J5-B: the optional minimum shadow age gates go-live (direct path too).
#[tokio::test]
async fn j5_min_shadow_age_gates_go_live() {
    for db in test_dbs().await {
        let (app, _mock_upstream) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        // SOLO-admin org: direct path (no defer) so the guard is what we
        // exercise, not the approval plumbing.
        let (_, body) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&token),
                &json!({"name": "Shadow Org", "slug": unique_slug("shadow")}),
            ),
        )
        .await;
        let org_id = body["id"].as_str().expect("org").to_string();
        let align = spawn_align_mock().await;

        // Configure auto_block (direct: solo org) ⇒ shadow starts now.
        let (status, body) = send(
            &app,
            put_classifier(&org_id, &token, &classifier_body(&align, "shadow journey")),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{}: {body}", db.name);

        // Opt into a 1-hour minimum shadow age.
        let (status, body) = send(&app, put_settings(&org_id, &token, 300, 3600)).await;
        assert_eq!(status, StatusCode::OK, "{}: settings: {body}", db.name);

        // Immediate go-live: 409 — the shadow window is seconds old.
        let (status, body) = send(
            &app,
            post_json(
                &format!("/api/v1/orgs/{org_id}/classifier/go-live"),
                Some(&token),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{}: too-young shadow must refuse: {body}",
            db.name
        );
        let (_, view) = send(
            &app,
            get_json(&format!("/api/v1/orgs/{org_id}/classifier"), Some(&token)),
        )
        .await;
        assert_eq!(
            view["auto_block_live"], false,
            "{}: refused go-live armed nothing: {view}",
            db.name
        );

        // Gate off (0): the same go-live executes.
        let (status, _) = send(&app, put_settings(&org_id, &token, 300, 0)).await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);
        let (status, body) = send(
            &app,
            post_json(
                &format!("/api/v1/orgs/{org_id}/classifier/go-live"),
                Some(&token),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{}: go-live after gate-off: {body}",
            db.name
        );
        let (_, view) = send(
            &app,
            get_json(&format!("/api/v1/orgs/{org_id}/classifier"), Some(&token)),
        )
        .await;
        assert_eq!(view["auto_block_live"], true, "{}: {view}", db.name);
    }
}

/// PUT /classifier (an actual PUT, unlike the harness's post helpers).
fn put_classifier(
    org_id: &str,
    token: &str,
    body: &Value,
) -> axum::http::Request<axum::body::Body> {
    use axum::body::Body;
    use axum::http::Request;
    Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/orgs/{org_id}/classifier"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// PUT /settings with both knobs.
fn put_settings(
    org_id: &str,
    token: &str,
    window: i64,
    min_shadow: i64,
) -> axum::http::Request<axum::body::Body> {
    use axum::body::Body;
    use axum::http::Request;
    Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/orgs/{org_id}/settings"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({"concurrency_window_secs": window, "classifier_min_shadow_secs": min_shadow})
                .to_string(),
        ))
        .unwrap()
}
