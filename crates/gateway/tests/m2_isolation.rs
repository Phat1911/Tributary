//! SPEC §14 M2 "done when": **Two pools isolated: one's quota never affects
//! the other.** Full stack — org/pool/provider-key/member/pool-key created
//! through the control API, consumption through `/v1/chat/completions`
//! against the mock upstream. Also proves rule 4: a denied request never
//! reaches upstream, and the 429 carries a reset hint.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use http_body_util::BodyExt;
use serde_json::Value;
use serde_json::json;
use tower::ServiceExt;
use tributary_gateway::db;

/// Fires a control-plane request with the admin session.
fn api(method: &str, uri: &str, token: &str, body: Option<&Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

/// Send + (status, body). Panics on non-2xx unless `expect_error` is set.
async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(req).await.expect("oneshot");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// One fully-provisioned pool reached entirely through the control API.
struct LivePool {
    pool_id: String,
    member_id: String,
    provider_key_id: String,
    /// The issued key's row id (for reading its usage events).
    key_row_id: String,
    /// The raw key — shown exactly once by the control API.
    key_raw: String,
}

/// Provisions org → pool → provider key → member → member pool key.
async fn provision_pool(
    app: &axum::Router,
    token: &str,
    mock: &MockUpstream,
    name: &str,
) -> LivePool {
    let (_, org) = call(
        app,
        api(
            "POST",
            "/api/v1/orgs",
            token,
            Some(&json!({"name": name, "slug": format!("{}-{}", name, db::new_id())})),
        ),
    )
    .await;
    let (_, pool) = call(
        app,
        api(
            "POST",
            "/api/v1/pools",
            token,
            Some(&json!({
                "org_id": org["id"].as_str().unwrap(),
                "name": format!("{name}-pool"),
                "model_allowlist": ["m-test"],
            })),
        ),
    )
    .await;
    let pool_id = pool["id"].as_str().unwrap().to_string();

    let (_, pk) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/provider-keys"),
            token,
            Some(&json!({"key": TEST_UPSTREAM_KEY, "base_url": mock.base_url})),
        ),
    )
    .await;
    assert_eq!(pk["health"], "ok", "mock validates the provider key");

    let (_, member) = call(
        app,
        api(
            "POST",
            "/api/v1/members",
            token,
            Some(&json!({
                "org_id": org["id"].as_str().unwrap(),
                "email": format!("{name}-{}@t.dev", db::new_id()),
                "password": "password123",
            })),
        ),
    )
    .await;
    let member_id = member["id"].as_str().unwrap().to_string();

    let (_, key) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/keys"),
            token,
            Some(&json!({"user_id": member_id})),
        ),
    )
    .await;
    LivePool {
        pool_id,
        member_id,
        provider_key_id: pk["id"].as_str().unwrap().to_string(),
        key_row_id: key["id"].as_str().unwrap().to_string(),
        key_raw: key["raw"].as_str().unwrap().to_string(),
    }
}

/// The done-when scenario: token-quota exhaustion in pool A leaves pool B
/// fully serving, and pool A's denied requests never touch upstream.
#[tokio::test]
async fn two_pools_isolated_token_quota_exhaustion_never_blocks_the_other() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await; // 18 tokens per call
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;

        let a = provision_pool(&app, &token, &mock, "alpha").await;
        let b = provision_pool(&app, &token, &mock, "beta").await;

        // Member-of-A token quota: one mock call = 18 tokens (11+7), so a
        // limit of 18 exhausts exactly after the first request.
        let (status, _) = call(
            &app,
            api(
                "POST",
                "/api/v1/quota-rules",
                &token,
                Some(&json!({
                    "subject_kind": "user",
                    "subject_id": a.member_id,
                    "period": "day",
                    "limit_tokens": 18,
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{}", test_db.name);

        // A #1: served and metered.
        let (status, body) = call(&app, chat_request(Some(&a.key_raw), &chat_body())).await;
        assert_eq!(status, StatusCode::OK, "{}: {body}", test_db.name);

        // A #2: 429 BEFORE upstream (rule 4) with a reset hint.
        let response = app
            .clone()
            .oneshot(chat_request(Some(&a.key_raw), &chat_body()))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "{}",
            test_db.name
        );
        assert!(
            response.headers().get(header::RETRY_AFTER).is_some(),
            "429 must carry Retry-After"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error["error"]["type"], "rate_limit_error");
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("18 tokens per day"),
            "reset hint names the limit: {error}"
        );

        // Isolation: B serves repeatedly despite A's exhaustion.
        for n in 0..3 {
            let (status, body) = call(&app, chat_request(Some(&b.key_raw), &chat_body())).await;
            assert_eq!(status, StatusCode::OK, "B call #{n}: {body}");
        }

        // Rule-4 proof: A's denied request never reached upstream. Total
        // upstream calls = 1 (A#1) + 3 (B) — the 429 added nothing.
        let captured = mock.captured.lock().unwrap().len();
        assert_eq!(
            captured, 4,
            "{}: denied requests must not hit upstream",
            test_db.name
        );

        // Attribution: usage rows carry their pool + provider key.
        let rows_a = db::usage_events_for_key(&test_db.pool, &a.key_row_id)
            .await
            .unwrap();
        assert_eq!(rows_a.len(), 1, "only A's first call was metered");
        assert_eq!(rows_a[0].pool_id.as_deref(), Some(a.pool_id.as_str()));
        assert_eq!(
            rows_a[0].provider_key_id.as_deref(),
            Some(a.provider_key_id.as_str())
        );
        let rows_b = db::usage_events_for_key(&test_db.pool, &b.key_row_id)
            .await
            .unwrap();
        assert_eq!(rows_b.len(), 3);
        assert_eq!(rows_b[0].pool_id.as_deref(), Some(b.pool_id.as_str()));
        assert_eq!(
            rows_b[0].provider_key_id.as_deref(),
            Some(b.provider_key_id.as_str())
        );
        assert_ne!(rows_a[0].pool_id, rows_b[0].pool_id, "walls hold");
    }
}

/// Request-rate limits (per pool) fire on the in-process fast path before
/// any upstream work, and never affect an unruled pool.
#[tokio::test]
async fn request_rate_limit_fires_before_upstream_and_is_pool_scoped() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;

        let c = provision_pool(&app, &token, &mock, "gamma").await;
        let d = provision_pool(&app, &token, &mock, "delta").await;

        let (status, _) = call(
            &app,
            api(
                "POST",
                "/api/v1/quota-rules",
                &token,
                Some(&json!({
                    "subject_kind": "pool",
                    "subject_id": c.pool_id,
                    "period": "minute",
                    "limit_requests": 1,
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{}", test_db.name);

        // C #1 passes, C #2 is rate-limited before upstream.
        let (status, _) = call(&app, chat_request(Some(&c.key_raw), &chat_body())).await;
        assert_eq!(status, StatusCode::OK);
        let response = app
            .clone()
            .oneshot(chat_request(Some(&c.key_raw), &chat_body()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("1 requests per minute"),
            "{error}"
        );

        // D unaffected; upstream saw exactly one C call plus D's.
        let (status, _) = call(&app, chat_request(Some(&d.key_raw), &chat_body())).await;
        assert_eq!(status, StatusCode::OK);
        let captured = mock.captured.lock().unwrap().len();
        assert_eq!(captured, 2, "denied C#2 must not reach upstream");
    }
}

/// Model allowlists are house rules enforced locally (SPEC §3.1).
#[tokio::test]
async fn model_outside_allowlist_is_rejected_locally() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let p = provision_pool(&app, &token, &mock, "epsilon").await; // ["m-test"]

        let mut body = chat_body();
        body["model"] = json!("m-forbidden");
        let response = app
            .clone()
            .oneshot(chat_request(Some(&p.key_raw), &body))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{}",
            test_db.name
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("m-forbidden")
        );
        assert!(
            mock.captured.lock().unwrap().is_empty(),
            "disallowed model must not reach upstream"
        );
    }
}

// --- helpers ----

/// Bootstraps the host admin and logs in through the real HTTP surface.
async fn bootstrap_admin_and_login(pool: &db::Db, app: &axum::Router) -> String {
    tributary_control::create_host_admin(pool, "root@tributary.dev", "hunter2hunter2")
        .await
        .expect("bootstrap");
    let (_, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"email": "root@tributary.dev", "password": "hunter2hunter2"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}
