//! SPEC §14 M4 "done when" journeys, walked through the PUBLIC API surface
//! only (workflow step 12 — born from the M3 retro: piecewise verification
//! cannot prove a journey). Test setup provisions exclusively via the
//! control API + the product's own host-admin bootstrap; NOTHING is seeded
//! directly into the DB. Direct DB reads appear only in ASSERTIONS (the
//! email outbox has no public endpoint yet), never in setup.
//!
//! - **J1 leak demo:** plant honey key via API → it rides the shared
//!   listing shape-identically → use it at the gateway → generic 401
//!   byte-identical to an unknown key + email alert + audit row + review
//!   flag, and it NEVER serves traffic.
//! - **J2 canary alias:** canary model in the member's allowlist → request
//!   targets it → denied locally (same 400 as any allowlist reject) +
//!   alert + audit + flag; upstream untouched.
//! - **J3 classifier:** org configures classifier (probe-on-save) +
//!   purpose → off-purpose request → flagged in the review queue with a
//!   score and NO prompt text; auto-block is opt-in + shadow-first ("would
//!   have blocked N"), then live: key SUSPENDED pending one-click unblock;
//!   member disclosure banner active in Alert mode and stronger when live.
//! - **J4 IP allowlist:** admin sets a CIDR on a key → outside source is
//!   denied + audited; inside source passes; empty allowlist = dormant.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use tributary_gateway::db;

/// Fires a control-plane request with a bearer token.
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

/// Send + (status, parsed body). Panics on transport failure.
async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let (status, bytes) = call_raw(app, req).await;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// Send + (status, raw bytes) — J1 needs byte-level identity.
async fn call_raw(app: &axum::Router, req: Request<Body>) -> (StatusCode, axum::body::Bytes) {
    let response = app.clone().oneshot(req).await.expect("oneshot");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, bytes)
}

/// A chat request with a chosen key, optional XFF, and optional model.
fn chat_req(key: &str, xff: Option<&str>, body: &Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(xff) = xff {
        builder = builder.header("x-forwarded-for", xff);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// Polls an async predicate (detection writes, classifier scoring) until it
/// holds or the deadline passes — fails the test with `what` on timeout.
async fn wait_for<F, Fut>(what: &str, mut pred: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..400 {
        if pred().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Bootstraps the host admin via the product's own path (CLI-equivalent)
/// and logs in through the real HTTP surface.
async fn bootstrap_admin_and_login(pool: &db::Db, app: &axum::Router) -> String {
    let _ =
        tributary_control::create_host_admin(pool, "root@tributary.dev", "hunter2hunter2").await;
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
    body["token"].as_str().expect("login token").to_string()
}

/// The fully API-provisioned stack every journey starts from: org → pool
/// (allowlist m-test) → provider key (mock upstream) → member → member key.
struct Stack {
    org_id: String,
    pool_id: String,
    member_email: String,
    key_id: String,
    key_raw: String,
}

/// Provisions the stack via the public API (workflow step 12: no seeding).
async fn provision_stack(app: &axum::Router, token: &str, mock: &MockUpstream) -> Stack {
    let unique = db::new_id();
    let (_, org) = call(
        app,
        api(
            "POST",
            "/api/v1/orgs",
            token,
            Some(&json!({"name": "Journey", "slug": format!("journey-{unique}")})),
        ),
    )
    .await;
    let org_id = org["id"].as_str().unwrap().to_string();

    let (_, pool) = call(
        app,
        api(
            "POST",
            "/api/v1/pools",
            token,
            Some(&json!({
                "org_id": org_id,
                "name": "journey-pool",
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
    assert_eq!(
        pk["health"], "ok",
        "mock upstream validates the provider key"
    );

    let member_email = format!("member-{unique}@t.dev");
    let (_, member) = call(
        app,
        api(
            "POST",
            "/api/v1/members",
            token,
            Some(&json!({
                "org_id": org_id,
                "email": member_email,
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
    Stack {
        org_id,
        pool_id,
        member_email,
        key_id: key["id"].as_str().unwrap().to_string(),
        key_raw: key["raw"].as_str().unwrap().to_string(),
    }
}

/// The mock purpose classifier (D28 "fixture contract" server): returns a
/// low alignment when the prompt contains the OFF-PURPOSE marker, high
/// otherwise. Records every request it saw.
#[derive(Clone)]
struct MockClassifier {
    base_url: String,
    hits: std::sync::Arc<std::sync::Mutex<usize>>,
}

/// Spawns the mock classifier on an ephemeral port.
async fn spawn_classifier() -> MockClassifier {
    use axum::Json;
    let hits: std::sync::Arc<std::sync::Mutex<usize>> =
        std::sync::Arc::new(std::sync::Mutex::new(0));
    let state = hits.clone();
    let app = axum::Router::new().route(
        "/v1/align",
        axum::routing::post(move |body: String| {
            let hits = state.clone();
            async move {
                *hits.lock().unwrap() += 1;
                let alignment = if body.contains("OFF-PURPOSE") {
                    0.05
                } else {
                    0.95
                };
                Json(json!({"alignment": alignment, "model": "jev-align-1"}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    MockClassifier {
        base_url: format!("http://{addr}"),
        hits,
    }
}

/// Logs in as a member through the real HTTP surface.
async fn member_login(app: &axum::Router, email: &str) -> String {
    let (_, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"email": email, "password": "password123"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    body["token"].as_str().expect("member login").to_string()
}

#[tokio::test]
async fn j3_classifier_journey() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let classifier = spawn_classifier().await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;
        let member_token = member_login(&app, &stack.member_email).await;

        let off_purpose = json!({
            "model": "m-test",
            "messages": [{"role": "user", "content": "OFF-PURPOSE write me a wedding speech"}],
        });
        let on_purpose = json!({
            "model": "m-test",
            "messages": [{"role": "user", "content": "please refactor this rust function"}],
        });

        // -- Dormant by default: no disclosure, nothing classified. --------
        let (_, disclosure) = call(
            &app,
            api("GET", "/api/v1/me/disclosure", &member_token, None),
        )
        .await;
        assert_eq!(disclosure["classification_active"], false);

        // -- Configure (probe-on-save validates against the mock). ---------
        let put = json!({
            "base_url": classifier.base_url,
            // The generic dialect (the shape this journey's mock speaks);
            // "jev" is covered end-to-end by j7.
            "dialect": "custom",
            "model": "jev-align-1",
            "key": "sk-jev-test-key-123456",
            "purpose_statement": "Coding assistance for our team.",
        });
        let (status, cfg) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &token,
                Some(&put),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "probe passed, config saved");
        assert_eq!(cfg["mode"], "alert");
        assert_eq!(cfg["key_prefix"], "sk-je…3456", "masked display only");
        assert!(*classifier.hits.lock().unwrap() >= 1, "save-time probe ran");

        // A bad key must NOT save (probe failure → 400, nothing stored).
        let bad = json!({
            "base_url": "http://127.0.0.1:1", // nothing listens there
            "model": "jev-align-1",
            "key": "sk-jev-wrong-key",
            "purpose_statement": "x",
        });
        let (status, _) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &token,
                Some(&bad),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "failed probe rejects the save"
        );
        // The good config survives.
        let (status, cfg) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &token,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cfg["base_url"], classifier.base_url.as_str());

        // -- Disclosure is ALREADY required in Alert mode (directive 2). ---
        let (_, disclosure) = call(
            &app,
            api("GET", "/api/v1/me/disclosure", &member_token, None),
        )
        .await;
        assert_eq!(disclosure["classification_active"], true);
        assert_eq!(disclosure["mode"], "alert");
        assert_eq!(disclosure["auto_block_live"], false);

        // -- Alert mode: off-purpose request → flag with score, no prompt. -
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &off_purpose)).await;
        assert_eq!(status, StatusCode::OK, "alert mode never blocks");
        wait_for("classifier_flag review flag with score", || {
            let app = app.clone();
            let token = token.clone();
            // Scoped like every wait in this file: only THIS key's flag
            // proves the journey progressed.
            let key_id = stack.key_id.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "classifier_flag"
                            && r["pool_key_id"] == key_id.as_str()
                            && r["score_bp"].as_i64().is_some()
                            && r["score_bp"].as_i64().unwrap() < 5000
                    })
                })
            }
        })
        .await;
        // The flag detail must NOT contain the prompt (D28: scores, not
        // prompts) — the marker word appears nowhere in the payload.
        let (_, flags) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let flag_json = flags.to_string();
        assert!(
            !flag_json.contains("OFF-PURPOSE"),
            "no prompt text in flags"
        );
        assert!(!flag_json.contains("wedding"), "no prompt content in flags");
        // The member's key is untouched in Alert mode.
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &on_purpose)).await;
        assert_eq!(status, StatusCode::OK);

        // -- Auto-block: opt-in requires acknowledgement (the warning). ----
        let auto = json!({
            "base_url": classifier.base_url,
            "dialect": "custom",
            "model": "jev-align-1",
            "key": "sk-jev-test-key-123456",
            "purpose_statement": "Coding assistance for our team.",
            "mode": "auto_block",
        });
        let (status, err) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &token,
                Some(&auto),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "unacknowledged auto-block rejected"
        );
        assert!(
            err["error"]["message"]
                .as_str()
                .unwrap()
                .contains("acknowledge"),
            "the rejection IS the warning"
        );

        // Acknowledged → SHADOW window starts (never straight to live).
        let mut acknowledged = auto.clone();
        acknowledged["acknowledge_auto_block"] = json!(true);
        let (status, cfg) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &token,
                Some(&acknowledged),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(cfg["mode"], "auto_block");
        assert_eq!(cfg["auto_block_live"], false, "shadow first (D28)");
        assert!(cfg["shadow_started_at"].as_i64().is_some());

        // Shadow: off-purpose flows THROUGH, but the counter says "would".
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &off_purpose)).await;
        assert_eq!(status, StatusCode::OK, "shadow never blocks");
        wait_for("shadow would_have_blocked >= 1", || {
            let app = app.clone();
            let token = token.clone();
            let org = stack.org_id.clone();
            async move {
                let (_, cfg) = call(
                    &app,
                    api(
                        "GET",
                        &format!("/api/v1/orgs/{org}/classifier"),
                        &token,
                        None,
                    ),
                )
                .await;
                cfg["shadow_would_have_blocked"].as_i64().unwrap_or(0) >= 1
            }
        })
        .await;
        // Key still live during shadow.
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &on_purpose)).await;
        assert_eq!(status, StatusCode::OK);

        // -- Go live → off-purpose SUSPENDS the key pending review. --------
        let (status, cfg) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{}/classifier/go-live", stack.org_id),
                &token,
                Some(&json!({})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cfg["auto_block_live"], true);

        // Disclosure strengthens when live (directive 2).
        let (_, disclosure) = call(
            &app,
            api("GET", "/api/v1/me/disclosure", &member_token, None),
        )
        .await;
        assert_eq!(disclosure["auto_block_live"], true);

        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &off_purpose)).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the triggering request itself completes"
        );
        wait_for("classifier_auto_block flag + suspension", || {
            let app = app.clone();
            let token = token.clone();
            // Scoped to THIS key: a stale auto-block flag from an earlier
            // run in the shared Postgres test DB must not satisfy the
            // wait before THIS run's (async) suspension has landed —
            // the unscoped check was a latent flake on re-run.
            let key_id = stack.key_id.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "classifier_auto_block" && r["pool_key_id"] == key_id.as_str()
                    })
                })
            }
        })
        .await;
        let (status, err) = call(&app, chat_req(&stack.key_raw, None, &on_purpose)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "suspended key denied");
        assert!(
            err["error"]["message"]
                .as_str()
                .unwrap()
                .contains("suspended pending review"),
            "the member is told why"
        );

        // -- One-click unblock: resolve the flag, key works again. ---------
        let (_, flags) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let flag_id = flags
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "classifier_auto_block" && r["pool_key_id"] == stack.key_id)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/review-flags/{flag_id}/resolve"),
                &token,
                Some(&json!({"action": "unblock"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &on_purpose)).await;
        assert_eq!(status, StatusCode::OK, "unblocked key serves again");
    }
}

// ---------------------------------------------------------------------------
// J4 — IP allowlist journey
// ---------------------------------------------------------------------------

#[tokio::test]
async fn j4_ip_allowlist_journey() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // No allowlist → any source works (dormant default).
        let (status, _) = call(
            &app,
            chat_req(&stack.key_raw, Some("203.0.113.7"), &chat_body()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // Admin pins the key to the office network.
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/allowlists",
                    stack.pool_id, stack.key_id
                ),
                &token,
                Some(&json!({"cidr": "10.0.0.0/8", "note": "office"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        // Garbage CIDRs are rejected at the API (never stored).
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/allowlists",
                    stack.pool_id, stack.key_id
                ),
                &token,
                Some(&json!({"cidr": "not-a-cidr"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Outside → denied + audited; inside → served.
        let before = mock.captured_count();
        let (status, err) = call(
            &app,
            chat_req(&stack.key_raw, Some("203.0.113.7"), &chat_body()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            err["error"]["message"]
                .as_str()
                .unwrap()
                .contains("restricted to configured IP ranges")
        );
        assert_eq!(
            mock.captured_count(),
            before,
            "denied locally, no upstream call"
        );

        let (status, _) = call(
            &app,
            chat_req(&stack.key_raw, Some("10.42.0.1"), &chat_body()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "in-range source passes");

        // Direct (proxy-less) connections fail CLOSED: "local" matches no
        // CIDR — documented behavior for allowlisted keys.
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &chat_body())).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // The audit row is the J4 proof.
        wait_for("security.ip_allowlist_denied audit row", || {
            let app = app.clone();
            let token = token.clone();
            async move {
                let (_, audit) =
                    call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
                audit.as_array().is_some_and(|rows| {
                    rows.iter().any(|r| {
                        r["action"] == "security.ip_allowlist_denied"
                            && r["after"]["ip"] == "203.0.113.7"
                    })
                })
            }
        })
        .await;

        // Removing the CIDR returns the key to dormant (any source).
        let (_, entries) = call(
            &app,
            api(
                "GET",
                &format!(
                    "/api/v1/pools/{}/keys/{}/allowlists",
                    stack.pool_id, stack.key_id
                ),
                &token,
                None,
            ),
        )
        .await;
        // NOTE: allowlists list rides the management surface; delete by id.
        let entry_id = entries[0]["id"].as_str().unwrap().to_string();
        let (status, _) = call(
            &app,
            api(
                "DELETE",
                &format!(
                    "/api/v1/pools/{}/keys/{}/allowlists/{}",
                    stack.pool_id, stack.key_id, entry_id
                ),
                &token,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &app,
            chat_req(&stack.key_raw, Some("203.0.113.7"), &chat_body()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "empty allowlist is dormant again");
    }
}

/// Amended M4 done-when (D31): "quota cap demonstrated in audit log".
/// A cap that fires invisibly is indistinguishable from an outage — the
/// 429 must leave an audit row (no flag, no email: a cap working is house
/// rules, not a leak signal). Everything below is public-API provisioning.
#[tokio::test]
async fn j5_quota_cap_audited() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // A day cap tighter than the mock's per-request usage (11+7 tokens).
        let (status, _) = call(
            &app,
            api(
                "POST",
                "/api/v1/quota-rules",
                &token,
                Some(&json!({
                    "subject_kind": "pool",
                    "subject_id": stack.pool_id,
                    "period": "day",
                    "limit_tokens": 10
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let body = json!({"model": "m-test", "messages": [{"role": "user", "content": "hi"}]});
        // First request passes the check (0 used), burns 18 tokens.
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &body)).await;
        assert_eq!(status, StatusCode::OK);
        // Second request: over the cap → 429 BEFORE any upstream call.
        let before = mock.captured_count();
        let (status, err) = call(&app, chat_req(&stack.key_raw, None, &body)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{err}");
        assert_eq!(mock.captured_count(), before, "denied before upstream");

        // The audit trail shows the cap firing.
        wait_for("security.quota_denied audit row", || {
            let app = app.clone();
            let token = token.clone();
            async move {
                let (_, audit) =
                    call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
                audit
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|r| r["action"] == "security.quota_denied"))
            }
        })
        .await;
    }
}

/// D32: distinct-IP concurrency — same key from two different /24s inside
/// the window fires ONE alert-only flag (queue + audit + email), never
/// blocks the key. Director-directed founder feature; replaces the removed
/// canary sensors as the credential-sharing tripwire. Everything below is
/// public-API provisioning (workflow step 12).
#[tokio::test]
async fn j6_distinct_ip_concurrency() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        let body = json!({"model": "m-test", "messages": [{"role": "user", "content": "hi"}]});

        // Two sources in DIFFERENT /24s (91.207.174.x vs 203.0.113.x).
        // Same-network hosts must NOT trip it, so prove that first: two
        // requests from one /24 stay quiet.
        let (status, _) = call(&app, chat_req(&stack.key_raw, Some("91.207.174.9"), &body)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            &app,
            chat_req(&stack.key_raw, Some("91.207.174.254"), &body),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "same-/24 hosts are one source");

        // No flag yet.
        let (_, flags) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        // Scoped to THIS key: the shared Postgres test DB carries flags
        // from earlier runs (other orgs), and those must never fail a
        // fresh run — only this key's behavior is under test here.
        assert!(
            !flags.as_array().is_some_and(|f| {
                f.iter().any(|r| {
                    r["kind"] == "distinct_ip_concurrency"
                        && r["pool_key_id"] == stack.key_id.as_str()
                })
            }),
            "one network alone must not flag"
        );

        // Second network → flag fires, request still SUCCEEDS (alert-only).
        let (status, _) = call(&app, chat_req(&stack.key_raw, Some("203.0.113.7"), &body)).await;
        assert_eq!(status, StatusCode::OK, "D32 is alert-only, never blocks");

        wait_for("distinct_ip_concurrency flag", || {
            let app = app.clone();
            let token = token.clone();
            // Clone (not move): `stack` is borrowed again after the
            // closure, so only the one String rides along.
            let key_id = stack.key_id.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "distinct_ip_concurrency"
                            && r["pool_key_id"] == key_id.as_str()
                            && r["detail_json"]["shared_networks"]
                                .as_array()
                                .is_some_and(|n| n.len() >= 2)
                    })
                })
            }
        })
        .await;

        // De-duplication: MORE shared-key traffic inside the window does
        // not add a second flag (one alert per key per window).
        let (_, flags_before) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let count_before = flags_before
            .as_array()
            .map(|f| {
                f.iter()
                    .filter(|r| {
                        r["kind"] == "distinct_ip_concurrency"
                            && r["pool_key_id"] == stack.key_id.as_str()
                    })
                    .count()
            })
            .unwrap_or(0);
        let (status, _) = call(&app, chat_req(&stack.key_raw, Some("198.51.100.5"), &body)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, flags_after) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let count_after = flags_after
            .as_array()
            .map(|f| {
                f.iter()
                    .filter(|r| {
                        r["kind"] == "distinct_ip_concurrency"
                            && r["pool_key_id"] == stack.key_id.as_str()
                    })
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            count_before, count_after,
            "one alert per key per window, not one per request"
        );
    }
}
/// The systemone-dialect mock (director research 2026-10-21): speaks the
/// documented TypeSafe/Jev contract — score at `answers.alignment.noul`,
/// low when the OFF-PURPOSE marker appears in `state.message`. Captures
/// the last request body so the journey can assert what we SHIPPED to the
/// vendor, not just what came back.
#[derive(Clone)]
struct MockSystemone {
    base_url: String,
    hits: std::sync::Arc<std::sync::Mutex<usize>>,
    last_body: std::sync::Arc<std::sync::Mutex<serde_json::Value>>,
}

/// Spawns the systemone mock on an ephemeral port.
async fn spawn_systemone() -> MockSystemone {
    use axum::Json;
    let hits = std::sync::Arc::new(std::sync::Mutex::new(0));
    let last_body = std::sync::Arc::new(std::sync::Mutex::new(serde_json::Value::Null));
    let state = (hits.clone(), last_body.clone());
    let app = axum::Router::new().route(
        "/v1/systemone",
        axum::routing::post(move |body: String| {
            let (hits, last_body) = state.clone();
            async move {
                *hits.lock().unwrap() += 1;
                let parsed: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                *last_body.lock().unwrap() = parsed.clone();
                // The documented answer shape; the marker rides in
                // state.message, exactly where the dialect puts it.
                let noul = if parsed["state"]["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("OFF-PURPOSE"))
                {
                    0.05
                } else {
                    0.95
                };
                Json(json!({"answers": {"alignment": {"noul": noul}}}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    MockSystemone {
        base_url: format!("http://{addr}"),
        hits,
        last_body,
    }
}

/// J7: the REAL Jev dialect (TypeSafe systemone contract, director
/// research 2026-10-21). The version pin absorbed reality: this journey
/// proves the new adapter speaks the documented wire shape end-to-end —
/// save-time probe, scored flag, and the exact request body we ship.
#[tokio::test]
async fn j7_classifier_jev_dialect() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let systemone = spawn_systemone().await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // Configure with dialect "jev" — the probe is ONE minimal real
        // noul call against the configured endpoint.
        let put = json!({
            "dialect": "jev",
            "base_url": systemone.base_url,
            "model": "jev-latest",
            "key": "tsf-test-key-123456",
            "purpose_statement": "Coding assistance for our team.",
        });
        let (status, cfg) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &token,
                Some(&put),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "jev probe passed, config saved"
        );
        assert_eq!(cfg["dialect"], "jev");
        assert_eq!(*systemone.hits.lock().unwrap(), 1, "probe = one noul call");

        // The probe body must match the documented shape — this is the
        // contract assertion, at the wire level.
        {
            let body = systemone.last_body.lock().unwrap().clone();
            assert_eq!(body["model"], "jev-latest", "model travels in the body");
            assert_eq!(
                body["questions"]["alignment"]["type"], "noul",
                "noul-typed alignment question"
            );
            assert_eq!(
                body["state"]["message"], "Tributary save-time probe",
                "minimal probe text rides in state.message"
            );
            let instructions = body["questions"]["alignment"]["instructions"]
                .as_str()
                .unwrap();
            assert!(
                instructions.contains("Coding assistance for our team."),
                "purpose embedded in the instructions: {instructions}"
            );
        }

        // Off-purpose request through the real dialect → alert flag.
        let body = json!({"model": "m-test", "messages": [
            {"role": "user", "content": "OFF-PURPOSE via systemone"}
        ]});
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &body)).await;
        assert_eq!(status, StatusCode::OK);

        let key_id = stack.key_id.clone();
        let token_clone = token.clone();
        wait_for("jev-dialect classifier flag", || {
            let app = app.clone();
            let token = token_clone.clone();
            let key_id = key_id.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "classifier_flag"
                            && r["pool_key_id"] == key_id.as_str()
                            && r["score_bp"].as_i64().is_some_and(|bp| bp < 5000)
                    })
                })
            }
        })
        .await;

        // The scoring call shipped the prompt under state.message and the
        // flag detail carries scores — never the prompt text (D28).
        {
            let shipped = systemone.last_body.lock().unwrap().clone();
            assert!(
                shipped["state"]["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("OFF-PURPOSE via systemone")),
                "prompt text rides to the vendor in state.message"
            );
        }
        let (_, flags) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let flag_json = flags
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "classifier_flag" && r["pool_key_id"] == stack.key_id)
            .unwrap();
        assert!(
            !flag_json["detail_json"].to_string().contains("OFF-PURPOSE"),
            "the flag detail stores the score, never the prompt"
        );
    }
}

/// J8 — D33 admin four-eyes (workflow step 12; the milestone-close
/// journey). Walks the whole rule set through the PUBLIC API only:
/// dual-admin org → sensitive request defers (202, nothing executes) →
/// requester cannot self-approve (403, logged) → a DISTINCT admin
/// approves → the effect lands. Containment: a classifier alert-linked
/// suspension executes instantly with no pending action. Solo orgs
/// auto-pass with the logged marker.
#[tokio::test]
async fn j8_admin_four_eyes() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let classifier = spawn_classifier().await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let host = bootstrap_admin_and_login(&test_db.pool, &app).await;

        // -- Org with TWO admins: A + B, both seeded via the host role
        //    param (M3) — the product's only provisioning path.
        let unique = db::new_id();
        let (_, org) = call(
            &app,
            api(
                "POST",
                "/api/v1/orgs",
                &host,
                Some(&json!({"name": "FourEyes", "slug": format!("four-eyes-{unique}")})),
            ),
        )
        .await;
        let org_id = org["id"].as_str().unwrap().to_string();
        let email_a = format!("eyes-a-{unique}@t.dev");
        let email_b = format!("eyes-b-{unique}@t.dev");
        let (_, a) = call(
            &app,
            api(
                "POST",
                "/api/v1/members",
                &host,
                Some(&json!({
                    "org_id": org_id,
                    "email": email_a,
                    "password": "password123",
                    "role": "org_admin",
                })),
            ),
        )
        .await;
        assert_eq!(a["role"], "org_admin", "host seeds the second eye");
        let (_, b) = call(
            &app,
            api(
                "POST",
                "/api/v1/members",
                &host,
                Some(&json!({
                    "org_id": org_id,
                    "email": email_b,
                    "password": "password123",
                    "role": "org_admin",
                })),
            ),
        )
        .await;
        let _a_id = a["id"].as_str().unwrap().to_string();
        let b_id = b["id"].as_str().unwrap().to_string();
        let token_a = member_login(&app, &email_a).await;
        let token_b = member_login(&app, &email_b).await;

        // Routine stack BY ADMIN A (single-admin ops, no approvals): pool
        // → provider key → a key OWNED BY ADMIN B (the sensitive target).
        let (_, pool) = call(
            &app,
            api(
                "POST",
                "/api/v1/pools",
                &token_a,
                Some(&json!({
                    "org_id": org_id,
                    "name": "eyes-pool",
                    "model_allowlist": ["m-test"],
                })),
            ),
        )
        .await;
        let pool_id = pool["id"].as_str().unwrap().to_string();
        let (_, _pk) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/provider-keys"),
                &token_a,
                Some(&json!({"key": TEST_UPSTREAM_KEY, "base_url": mock.base_url})),
            ),
        )
        .await;
        let (_, bkey) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys"),
                &token_a,
                Some(&json!({"user_id": b_id})),
            ),
        )
        .await;
        let bkey_id = bkey["id"].as_str().unwrap().to_string();
        let bkey_raw = bkey["raw"].as_str().unwrap().to_string();

        // -- LEG 1: A expires B's key → PENDING, nothing executes. ------
        let expires_at = db::now_ms() + 3 * 24 * 3600 * 1000;
        let (status, pending) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys/{bkey_id}/expiry"),
                &token_a,
                Some(&json!({"expires_at": expires_at})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "sensitive action defers");
        assert_eq!(pending["kind"], "admin_key_expiry");
        assert_eq!(pending["status"], "pending");
        // The approvals card must NAME the target (founder UX report
        // 2026-10-21): whose key, which key, to when — not a bare kind.
        let summary = pending["summary"].as_str().unwrap_or_default();
        assert!(
            summary.contains(&format!("owner {email_b}")),
            "summary names the owner: got {summary:?}"
        );
        assert!(
            summary.contains("Set expiry 2026-"),
            "summary carries the date: got {summary:?}"
        );
        let action_id = pending["id"].as_str().unwrap().to_string();

        let (_, keys) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{pool_id}/keys/manage"),
                &token_a,
                None,
            ),
        )
        .await;
        let bkey_row = keys
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == json!(bkey_id))
            .unwrap();
        assert!(
            bkey_row["expires_at"].is_null(),
            "execution waits for the second eye — B's key untouched"
        );

        let (_, pending_list) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org_id}/pending-actions?status=pending"),
                &token_a,
                None,
            ),
        )
        .await;
        assert!(
            pending_list
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == json!(action_id)),
            "the queue surfaces the deferred action"
        );

        // -- LEG 2: A cannot self-approve (403, and the refusal is
        //    itself audited). -----------------------------------------
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/pending-actions/{action_id}/approve"),
                &token_a,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "actor ≠ approver (D33)");
        wait_for("self-approve denial audited", || {
            let app = app.clone();
            let token_a = token_a.clone();
            let action_id = action_id.clone();
            async move {
                let (_, audit) =
                    call(&app, api("GET", "/api/v1/audit?limit=200", &token_a, None)).await;
                audit.as_array().unwrap().iter().any(|r| {
                    r["action"] == "approval.self_approve_denied"
                        && r["target_id"] == json!(action_id)
                })
            }
        })
        .await;
        let (_, still) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org_id}/pending-actions?status=pending"),
                &token_b,
                None,
            ),
        )
        .await;
        assert!(
            still
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == json!(action_id)),
            "denied self-approval leaves the action pending"
        );

        // -- LEG 3: B (the distinct admin) approves → executes. ---------
        let (status, decided) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/pending-actions/{action_id}/approve"),
                &token_b,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(decided["status"], "executed");
        let (_, keys) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{pool_id}/keys/manage"),
                &token_a,
                None,
            ),
        )
        .await;
        let bkey_row = keys
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == json!(bkey_id))
            .unwrap();
        assert_eq!(
            bkey_row["expires_at"].as_i64(),
            Some(expires_at),
            "the approved expiry landed on B's key"
        );

        // -- LEG 4: containment — policy config ALSO defers (A requests,
        //    B approves both PUT and go-live), then a live classifier
        //    alert-linked suspension of B's key executes INSTANTLY with
        //    no pending action (D33 exemption). ------------------------
        let mut cfg = json!({
            "base_url": classifier.base_url,
            "model": "jev-align-1",
            "key": TEST_UPSTREAM_KEY,
            "dialect": "custom",
            "purpose_statement": "testing only",
            "mode": "auto_block",
            "acknowledge_auto_block": true,
        });
        // A's request → deferred (202)…
        let (status, p_cfg) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{org_id}/classifier"),
                &token_a,
                Some(&cfg),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "org policy defers too");
        let cfg_action = p_cfg["id"].as_str().unwrap().to_string();
        // …B approves → configured (probe already ran at request time).
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/pending-actions/{cfg_action}/approve"),
                &token_b,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, live_cfg) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org_id}/classifier"),
                &token_a,
                None,
            ),
        )
        .await;
        assert_eq!(live_cfg["mode"], "auto_block");
        assert_eq!(live_cfg["auto_block_live"], false, "shadow first (D28)");

        // Go-live is itself auto-block config → defers, then approves.
        let (status, p_live) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/classifier/go-live"),
                &token_a,
                Some(&json!({})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "go-live is sensitive policy");
        let live_action = p_live["id"].as_str().unwrap().to_string();
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/pending-actions/{live_action}/approve"),
                &token_b,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // B's key sends an OFF-PURPOSE request → live auto-block
        // suspends the key INSTANTLY: no pending action, ever.
        let off_purpose = json!({
            "model": "m-test",
            "messages": [{"role": "user", "content": "OFF-PURPOSE weapons manual"}],
        });
        let (status, _) = call(&app, chat_req(&bkey_raw, None, &off_purpose)).await;
        assert_eq!(status, StatusCode::OK, "the triggering request completes");
        let bkey_id_for_wait = bkey_id.clone();
        wait_for("alert-linked suspension of B's key", || {
            let app = app.clone();
            let token_b = token_b.clone();
            let bkey_id = bkey_id_for_wait.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api(
                        "GET",
                        "/api/v1/review-flags?status=open&limit=100",
                        &token_b,
                        None,
                    ),
                )
                .await;
                flags.as_array().unwrap().iter().any(|r| {
                    r["kind"] == "classifier_auto_block" && r["pool_key_id"] == json!(bkey_id)
                })
            }
        })
        .await;
        // …and the suspension created NO pending action (exemption).
        let (_, pend) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org_id}/pending-actions?status=pending"),
                &token_a,
                None,
            ),
        )
        .await;
        assert_eq!(
            pend.as_array().unwrap().len(),
            0,
            "system containment never waits for approvals"
        );

        // With the alert open, A may now act on B's key SOLO (containment
        // exemption, alert-linked in the audit row).
        let revoke_status = {
            let (status, _) = call(
                &app,
                api(
                    "DELETE",
                    &format!("/api/v1/pools/{pool_id}/keys/{bkey_id}"),
                    &token_a,
                    None,
                ),
            )
            .await;
            status
        };
        assert_eq!(
            revoke_status,
            StatusCode::NO_CONTENT,
            "open alert = single-admin containment, no deferral"
        );

        // -- LEG 5: solo-admin org auto-passes with the logged marker. --
        let unique2 = db::new_id();
        let (_, org2) = call(
            &app,
            api(
                "POST",
                "/api/v1/orgs",
                &host,
                Some(&json!({"name": "Solo", "slug": format!("solo-{unique2}")})),
            ),
        )
        .await;
        let org2_id = org2["id"].as_str().unwrap().to_string();
        let email_solo = format!("solo-{unique2}@t.dev");
        let (_, _solo_admin) = call(
            &app,
            api(
                "POST",
                "/api/v1/members",
                &host,
                Some(&json!({
                    "org_id": org2_id,
                    "email": email_solo,
                    "password": "password123",
                    "role": "org_admin",
                })),
            ),
        )
        .await;
        let token_solo = member_login(&app, &email_solo).await;
        cfg["base_url"] = json!(classifier.base_url);
        let (status, cfg2) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{org2_id}/classifier"),
                &token_solo,
                Some(&cfg),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "solo org executes directly (never deadlocks)"
        );
        assert_eq!(cfg2["mode"], "auto_block");
        // No pending action was created…
        let (_, pend2) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org2_id}/pending-actions"),
                &token_solo,
                None,
            ),
        )
        .await;
        assert_eq!(pend2.as_array().unwrap().len(), 0);
        // …and the audit row carries the solo marker.
        wait_for("solo-admin auto-pass marker in audit", || {
            let app = app.clone();
            let token_solo = token_solo.clone();
            let org2_id = org2_id.clone();
            async move {
                let (_, audit) = call(
                    &app,
                    api("GET", "/api/v1/audit?limit=200", &token_solo, None),
                )
                .await;
                audit.as_array().unwrap().iter().any(|r| {
                    r["action"] == "classifier.configured"
                        && r["org_id"] == json!(org2_id)
                        && r["after"]["approval"]["mode"] == json!("solo-admin auto-pass")
                })
            }
        })
        .await;
    }
}
