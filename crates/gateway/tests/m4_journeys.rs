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
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "classifier_flag"
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
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags
                    .as_array()
                    .is_some_and(|f| f.iter().any(|r| r["kind"] == "classifier_auto_block"))
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
            .find(|r| r["kind"] == "classifier_auto_block")
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
