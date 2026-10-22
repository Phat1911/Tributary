//! SPEC §14 M4 "done when" journeys, walked through the PUBLIC API surface
//! only (workflow step 12 — born from the M3 retro: piecewise verification
//! cannot prove a journey). Test setup provisions exclusively via the
//! control API + the product's own host-admin bootstrap; NOTHING is seeded
//! directly into the DB. Direct DB reads appear only in ASSERTIONS (the
//! email outbox has no public endpoint yet), never in setup.
//!
//! - **J1/J2 (honey key + canary alias): REMOVED with D31** (2026-10-19,
//!   founder-tested decision — low-yield tripwires); their journeys were
//!   retired with the features. The credential-sharing tripwire is now
//!   J6's distinct-IP concurrency detector (D32).
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
use sqlx::Row; // try_get on raw assertion-only DB reads (outbox, payloads)
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
    // Retry the login (final-scan): against the shared Postgres test DB
    // a parallel test's winning INSERT can be uncommitted when we log in
    // — the control harness documents this exact race and retries 40x.
    for _ in 0..40 {
        let (_, body) = call(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"email": "root@tributary.dev", "password": "hunter2hunter2"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await;
        if let Some(tok) = body["token"].as_str() {
            return tok.to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("login never succeeded after 40 attempts")
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
        // Baseline (round-2): the save-time probe must run EXACTLY once.
        let probe_hits = *classifier.hits.lock().unwrap();
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
        // Strict (round-2): >= 1 could hide a double-probe billing
        // regression — the PUT probes exactly once.
        assert_eq!(
            *classifier.hits.lock().unwrap(),
            probe_hits + 1,
            "save-time probe ran exactly once"
        );

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
                cfg["shadow_would_have_blocked"]
                    .as_i64()
                    .expect("shadow counter present")
                    >= 1
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
        // Prove NOTHING was stored (final-scan): the 400 alone would
        // pass even if a partial write landed the bad row.
        let (_, entries_now) = call(
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
        let kept = entries_now.as_array().expect("allowlist array");
        assert_eq!(kept.len(), 1, "only the good CIDR is stored: {kept:?}");
        assert_eq!(kept[0]["cidr"], "10.0.0.0/8");

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
        let key_id = stack.key_id.clone();
        wait_for("security.ip_allowlist_denied audit row", || {
            let app = app.clone();
            let token = token.clone();
            let key_id = key_id.clone();
            async move {
                let (_, audit) =
                    call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
                audit.as_array().is_some_and(|rows| {
                    rows.iter().any(|r| {
                        // Key-scoped (final-scan): on the shared test DB
                        // an unscoped predicate is satisfied by stale
                        // rows from earlier runs — a false pass.
                        r["action"] == "security.ip_allowlist_denied"
                            && r["target_id"] == key_id
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
        let entry_id = entries
            .as_array()
            .expect("allowlist rows array (round-2: no panic-index)")[0]["id"]
            .as_str()
            .unwrap()
            .to_string();
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
        // Key-scoped, no polling needed (final-scan): the gateway AWAITS
        // the audit write before returning the 429 — a direct assert
        // cannot be satisfied by a stale row from an earlier run.
        let (_, audit) = call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
        let quota_rows =
            audit.as_array().expect("audit array").iter().filter(|r| {
                r["action"] == "security.quota_denied" && r["target_id"] == stack.key_id
            });
        assert_eq!(
            quota_rows.count(),
            1,
            "exactly one quota_denied row naming THIS key"
        );
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
                                // Strict (round-2): exactly the two
                                // /24s seen at flag time.
                                .is_some_and(|n| n.len() == 2)
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
        // Strict (final-scan): expect() not unwrap_or(0) — a 401/500
        // body used to make both sides 0 and the dedupe assert a
        // tautology.
        let count_before = flags_before
            .as_array()
            .expect("flags array")
            .iter()
            .filter(|r| {
                r["kind"] == "distinct_ip_concurrency" && r["pool_key_id"] == stack.key_id.as_str()
            })
            .count();
        assert_eq!(count_before, 1, "the sharing event flagged exactly once");
        let flag_id = flags_before
            .as_array()
            .expect("flags array")
            .iter()
            .find(|r| {
                r["kind"] == "distinct_ip_concurrency" && r["pool_key_id"] == stack.key_id.as_str()
            })
            .expect("the flag we just counted")["id"]
            .as_str()
            .unwrap()
            .to_string();
        let (status, _) = call(&app, chat_req(&stack.key_raw, Some("198.51.100.5"), &body)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, flags_after) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let count_after = flags_after
            .as_array()
            .expect("flags array")
            .iter()
            .filter(|r| {
                r["kind"] == "distinct_ip_concurrency" && r["pool_key_id"] == stack.key_id.as_str()
            })
            .count();
        assert_eq!(
            count_before, count_after,
            "one alert per key per window, not one per request"
        );

        // -- Three-surface proof (round-2 gap): the SAME event must exist
        //    as (1) queue flag ✓above, (2) an audit row linked by flag id,
        //    (3) an email-outbox row. Assertion-only DB reads are
        //    sanctioned by the harness header. -------------------------
        let (_, audit_rows) = call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
        let linked = audit_rows
            .as_array()
            .expect("audit array")
            .iter()
            .find(|r| {
                r["action"] == "security.distinct_ip_concurrency"
                    && r["target_id"] == json!(flag_id)
            })
            .unwrap_or_else(|| panic!("D32 audit row linking flag {flag_id} is missing"));
        assert_eq!(linked["actor_id"], "system", "system actor on D32 rows");
        // db.sql() translates the ? placeholder per engine (PG needs $n).
        let outbox_sql = test_db
            .pool
            .sql("SELECT body FROM email_alerts WHERE org_id = ?");
        let outbox: Vec<String> = sqlx::query(sqlx::AssertSqlSafe(outbox_sql.as_str()))
            .bind(&stack.org_id)
            .fetch_all(test_db.pool.pool())
            .await
            .expect("outbox query")
            .into_iter()
            .map(|r| r.try_get("body").expect("body"))
            .collect();
        assert!(
            !outbox.is_empty(),
            "the sharing event queued an email alert (outbox persists on fallback)"
        );
        assert!(
            outbox.iter().all(|b| !b.contains(&stack.key_raw)),
            "alert email carries the masked prefix only — never the raw key"
        );

        // -- v6 /64 semantics on a FRESH key (round-2 journey gap): same
        //    /64 is one source, different /64s are two. ------------------
        let (_, members) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/members?org_id={}", stack.org_id),
                &token,
                None,
            ),
        )
        .await;
        let member_id = members.as_array().expect("members")[0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let (_, k2) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/keys", stack.pool_id),
                &token,
                Some(&json!({ "user_id": member_id })),
            ),
        )
        .await;
        let k2_raw = k2["raw"].as_str().unwrap().to_string();
        let k2_id = k2["id"].as_str().unwrap().to_string();
        let (st, _) = call(&app, chat_req(&k2_raw, Some("2001:db8:1::1"), &body)).await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = call(&app, chat_req(&k2_raw, Some("2001:db8:1::ffff:0:1"), &body)).await;
        assert_eq!(st, StatusCode::OK, "same-/64 v6 hosts are one source");
        let (_, v6flags) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        assert!(
            !v6flags.as_array().unwrap().iter().any(|r| {
                r["kind"] == "distinct_ip_concurrency" && r["pool_key_id"] == json!(k2_id)
            }),
            "one /64 alone must not flag (v6)"
        );
        let (st, _) = call(&app, chat_req(&k2_raw, Some("2001:db8:2::1"), &body)).await;
        assert_eq!(st, StatusCode::OK, "D32 stays alert-only (v6 leg)");
        let k2_id_wait = k2_id.clone();
        wait_for("v6 distinct-/64 flag", || {
            let app = app.clone();
            let token = token.clone();
            let k2_id = k2_id_wait.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "distinct_ip_concurrency" && r["pool_key_id"] == k2_id.as_str()
                    })
                })
            }
        })
        .await;
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
        let a_id = a["id"].as_str().unwrap().to_string();
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
        // Date prefix only ("Set expiry 2…"): the exact date is pinned
        // by the unit tests in approvals.rs — hardcoding "2026-" here
        // would be a wall-clock time bomb (final-scan).
        assert!(
            summary.contains("Set expiry 2"),
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
        // Response shape since the final scan: {action, issued?} — the
        // decided row rides under `action`; `issued` carries reveal-once
        // material only for credential-minting kinds (rotate).
        assert_eq!(decided["action"]["status"], "executed");
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

        // The containment audit row LINKS the alert (j11, round-2 gap):
        // D33 makes a solo action without a linked alert a detectable
        // policy violation — prove the link exists.
        let (_, arows) = call(&app, api("GET", "/api/v1/audit?limit=200", &token_a, None)).await;
        let containment = arows
            .as_array()
            .expect("audit rows")
            .iter()
            .find(|r| r["target_id"] == json!(bkey_id))
            .expect("containment revoke is audited");
        assert_eq!(
            containment["after"]["approval"]["mode"], "containment",
            "solo action is marked containment"
        );
        let linked_alert = containment["after"]["approval"]["alert"]
            .as_str()
            .expect("the audit row names the authorizing alert");
        // It must be THIS org's open auto-block flag for THIS key.
        let (_, open_flags) = call(
            &app,
            api(
                "GET",
                "/api/v1/review-flags?status=open&limit=100",
                &token_a,
                None,
            ),
        )
        .await;
        assert!(
            open_flags.as_array().unwrap().iter().any(|r| {
                r["id"] == json!(linked_alert)
                    && r["kind"] == "classifier_auto_block"
                    && r["pool_key_id"] == json!(bkey_id)
            }),
            "the linked alert is the open auto-block flag for B's key"
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

        // -- LEG 6 (founder follow-up 2026-10-21): a LEGACY pending row —
        //    payload predating the owner_email field — must still name the
        //    affected admin: the email is resolved at READ time, the
        //    stored payload never rewritten. Seeded via the same insert
        //    the product's request path uses, minus the enrichment.
        // The control-plane insert (the gateway's db alias is its own).
        tributary_control::db::insert_pending_action(
            &test_db.pool,
            &db::new_id(),
            &org_id,
            "admin_key_revoke",
            &bkey_id,
            // Legacy shape: key id, NO owner_email (pre-enrichment rows).
            &format!(r#"{{"key_id": "{bkey_id}"}}"#),
            &a_id,
            db::now_ms(),
        )
        .await
        .expect("legacy row insert");
        let legacy_id = {
            let (_, rows) = call(
                &app,
                api(
                    "GET",
                    &format!("/api/v1/orgs/{org_id}/pending-actions?status=pending"),
                    &token_a,
                    None,
                ),
            )
            .await;
            let row = rows
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["kind"] == "admin_key_revoke")
                .expect("legacy revoke row listed");
            let summary = row["summary"].as_str().unwrap_or_default();
            assert!(
                summary.contains(&format!("owner {email_b}")),
                "legacy row heals at read time: got {summary:?}"
            );
            row["id"].as_str().unwrap().to_string()
        };
        // Clean up: B rejects it (also exercises reject on a legacy row).
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/pending-actions/{legacy_id}/reject"),
                &token_b,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "legacy row rejects fine");
    }
}

// ===========================================================================
// FINAL-SCAN ADVERSARIAL SUITE (2026-10-22). Five reviewer subagents
// attacked the M4 features (four-eyes, classifier, allowlist, rotation).
// These tests pin what they broke or found unguarded — every assert here
// was RED against the pre-fix code. None weaken an existing assert (rule 6).
// ===========================================================================

/// IDENTITY & SCOPE MATRIX + RESTART (D33 invariants 1+2): members never
/// decide; two sessions of the requester are still the requester; foreign
/// admins and spoofed org paths get 404 (no existence oracle); pending
/// rows survive a restart with ZERO side effects; the HOST is a valid
/// second eye; routine member-key actions never defer.
#[tokio::test]
async fn adv_d33_identity_scope_matrix_and_restart() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let host = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let host_id = {
            let (_, me) = call(&app, api("GET", "/api/v1/auth/me", &host, None)).await;
            me["id"].as_str().unwrap().to_string()
        };

        let unique = db::new_id();
        let (_, o1) = call(
            &app,
            api(
                "POST",
                "/api/v1/orgs",
                &host,
                Some(&json!({"name": "M1", "slug": format!("m1-{unique}")})),
            ),
        )
        .await;
        let org1 = o1["id"].as_str().unwrap().to_string();
        let (_, o2) = call(
            &app,
            api(
                "POST",
                "/api/v1/orgs",
                &host,
                Some(&json!({"name": "M2", "slug": format!("m2-{unique}")})),
            ),
        )
        .await;
        let org2 = o2["id"].as_str().unwrap().to_string();
        let mk_admin = |org: &str, email: &str| {
            let app = &app;
            let host = &host;
            let email = email.to_string();
            let org = org.to_string();
            async move {
                call(
                    app,
                    api(
                        "POST",
                        "/api/v1/members",
                        host,
                        Some(&json!({
                            "org_id": org, "email": email,
                            "password": "password123", "role": "org_admin"
                        })),
                    ),
                )
                .await
            }
        };
        let (_, a) = mk_admin(&org1, &format!("mx-a-{unique}@t.dev")).await;
        let _a_id = a["id"].as_str().unwrap().to_string();
        let (_, b) = mk_admin(&org1, &format!("mx-b-{unique}@t.dev")).await;
        let b_id = b["id"].as_str().unwrap().to_string();
        let (_, m) = call(
            &app,
            api(
                "POST",
                "/api/v1/members",
                &host,
                Some(&json!({
                    "org_id": org1, "email": format!("mx-m-{unique}@t.dev"),
                    "password": "password123", "role": "member"
                })),
            ),
        )
        .await;
        let m_id = m["id"].as_str().unwrap().to_string();
        let (_, c) = mk_admin(&org2, &format!("mx-c-{unique}@t.dev")).await;
        let _ = c;

        let ta = member_login(&app, &format!("mx-a-{unique}@t.dev")).await;
        let tb = member_login(&app, &format!("mx-b-{unique}@t.dev")).await;
        let tm = member_login(&app, &format!("mx-m-{unique}@t.dev")).await;
        let tc = member_login(&app, &format!("mx-c-{unique}@t.dev")).await;

        let (_, pool) = call(
            &app,
            api(
                "POST",
                "/api/v1/pools",
                &ta,
                Some(&json!({
                    "org_id": org1, "name": "mx",
                    "model_allowlist": ["m-test"]
                })),
            ),
        )
        .await;
        let pool_id = pool["id"].as_str().unwrap().to_string();
        let (_, bkey) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys"),
                &ta,
                Some(&json!({"user_id": b_id})),
            ),
        )
        .await;
        let bkey_id = bkey["id"].as_str().unwrap().to_string();

        let at = db::now_ms() + 86_400_000;
        let (st, pend) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys/{bkey_id}/expiry"),
                &ta,
                Some(&json!({"expires_at": at})),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::ACCEPTED);
        let action = pend["id"].as_str().unwrap().to_string();

        // MEMBER approve → 403 (extractor, before any handler logic).
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org1}/pending-actions/{action}/approve"),
                &tm,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "member must never decide");
        // SECOND SESSION of the requester: still the same person → 403.
        let ta2 = member_login(&app, &format!("mx-a-{unique}@t.dev")).await;
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org1}/pending-actions/{action}/approve"),
                &ta2,
                None,
            ),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "two sessions of the requester are the same person"
        );
        // CROSS-ORG admin → 404 (no existence oracle); spoofed org path
        // cannot reach another org's row either.
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org1}/pending-actions/{action}/approve"),
                &tc,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "cross-org approve");
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org2}/pending-actions/{action}/approve"),
                &ta,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "spoofed org_id path");
        // Foreign admin cannot even LIST the org's queue.
        let (st, _) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org1}/pending-actions"),
                &tc,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "queue is org-scoped");

        // RESTART (fresh app, same pool): the deferral persists with ZERO
        // side effects — the key is untouched, the row still pending.
        let app2 = test_app(test_db.pool.clone(), &mock).await;
        let (_, keys) = call(
            &app2,
            api(
                "GET",
                &format!("/api/v1/pools/{pool_id}/keys/manage"),
                &ta,
                None,
            ),
        )
        .await;
        let row = keys
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == json!(bkey_id))
            .unwrap();
        assert!(
            row["expires_at"].is_null(),
            "no side effect survived the deferral across restart"
        );
        let (_, pend2) = call(
            &app2,
            api(
                "GET",
                &format!("/api/v1/orgs/{org1}/pending-actions?status=pending"),
                &tb,
                None,
            ),
        )
        .await;
        assert!(
            pend2
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == json!(action)),
            "pending row persisted across restart"
        );

        // HOST is a valid second eye → executes; the host is the decider.
        let (st, decided) = call(
            &app2,
            api(
                "POST",
                &format!("/api/v1/orgs/{org1}/pending-actions/{action}/approve"),
                &host,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(decided["action"]["status"], "executed");
        assert_eq!(
            decided["action"]["decided_by"],
            json!(host_id),
            "decided_by names the HOST approver"
        );

        // ROUTINE contrast: a MEMBER's key expiry never defers.
        let (_, mkey) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys"),
                &ta,
                Some(&json!({"user_id": m_id})),
            ),
        )
        .await;
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{pool_id}/keys/{}/expiry",
                    mkey["id"].as_str().unwrap()
                ),
                &ta,
                Some(&json!({"expires_at": db::now_ms() + 86_400_000})),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::NO_CONTENT, "member-key expiry is routine");
    }
}

/// DECISION RACE (final-scan F2 fix — claim-first): approve ∥ reject on
/// the same row. Exactly one wins, and the WORLD must always match the
/// ROW: a rejected action must never leave its side effect behind.
#[tokio::test]
async fn adv_d33_race_reject_vs_execute_world_matches_row() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let host = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let unique = db::new_id();
        let (_, org) = call(
            &app,
            api(
                "POST",
                "/api/v1/orgs",
                &host,
                Some(&json!({"name": "R", "slug": format!("race-{unique}")})),
            ),
        )
        .await;
        let org_id = org["id"].as_str().unwrap().to_string();
        let mut admin_ids = Vec::new();
        let mut emails = Vec::new();
        for who in ["a", "b", "c"] {
            let email = format!("race-{who}-{unique}@t.dev");
            let (_, m) = call(
                &app,
                api(
                    "POST",
                    "/api/v1/members",
                    &host,
                    Some(&json!({
                        "org_id": org_id, "email": email,
                        "password": "password123", "role": "org_admin"
                    })),
                ),
            )
            .await;
            admin_ids.push(m["id"].as_str().unwrap().to_string());
            emails.push(email);
        }
        let ta = member_login(&app, &emails[0]).await;
        let tb = member_login(&app, &emails[1]).await;
        let tc = member_login(&app, &emails[2]).await;

        let (_, pool) = call(
            &app,
            api(
                "POST",
                "/api/v1/pools",
                &ta,
                Some(&json!({
                    "org_id": org_id, "name": "rp", "model_allowlist": ["m-test"]
                })),
            ),
        )
        .await;
        let pool_id = pool["id"].as_str().unwrap().to_string();
        let (_, bkey) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys"),
                &ta,
                Some(&json!({"user_id": admin_ids[1]})),
            ),
        )
        .await;
        let bkey_id = bkey["id"].as_str().unwrap().to_string();

        for round in 0..5i64 {
            let at = db::now_ms() + (round + 2) * 86_400_000;
            let (st, pend) = call(
                &app,
                api(
                    "POST",
                    &format!("/api/v1/pools/{pool_id}/keys/{bkey_id}/expiry"),
                    &ta,
                    Some(&json!({"expires_at": at})),
                ),
            )
            .await;
            assert_eq!(st, StatusCode::ACCEPTED);
            let action = pend["id"].as_str().unwrap().to_string();

            let app1 = app.clone();
            let app2 = app.clone();
            let tc_r = tc.clone();
            let tb_r = tb.clone();
            let u1 = format!("/api/v1/orgs/{org_id}/pending-actions/{action}/approve");
            let u2 = format!("/api/v1/orgs/{org_id}/pending-actions/{action}/reject");
            let (r_approve, r_reject) = tokio::join!(
                async move { call(&app1, api("POST", &u1, &tc_r, None)).await },
                async move { call(&app2, api("POST", &u2, &tb_r, None)).await },
            );
            // Exactly one decision may win; the loser conflicts cleanly.
            let winners = [r_approve.0, r_reject.0]
                .iter()
                .filter(|s| **s == StatusCode::OK)
                .count();
            assert_eq!(
                winners, 1,
                "round {round}: exactly one winner (approve={:?} reject={:?})",
                r_approve.0, r_reject.0
            );

            // WORLD ⇔ ROW consistency (the F2 invariant).
            let (_, keys) = call(
                &app,
                api(
                    "GET",
                    &format!("/api/v1/pools/{pool_id}/keys/manage"),
                    &ta,
                    None,
                ),
            )
            .await;
            let row = keys
                .as_array()
                .unwrap()
                .iter()
                .find(|k| k["id"] == json!(bkey_id))
                .unwrap();
            let mut decided = None;
            for st_name in ["executed", "rejected"] {
                let (_, rows) = call(
                    &app,
                    api(
                        "GET",
                        &format!("/api/v1/orgs/{org_id}/pending-actions?status={st_name}"),
                        &ta,
                        None,
                    ),
                )
                .await;
                if let Some(r) = rows
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["id"] == json!(action))
                {
                    decided = Some(r.clone());
                }
            }
            let decided = decided.expect("row reached a terminal status");
            match decided["status"].as_str().unwrap() {
                "rejected" => assert!(
                    row["expires_at"].as_i64() != Some(at),
                    "round {round}: REJECTED but the expiry landed — side-effect-before-claim (F2)"
                ),
                "executed" => assert_eq!(row["expires_at"].as_i64(), Some(at)),
                other => panic!("round {round}: unexpected status {other}"),
            }
        }
    }
}

/// MUTUAL DEMOTE TOCTOU (final-scan F3 fix — locked, guarded UPDATE): A
/// requests demote(B), B requests demote(A), each approves the other's
/// CONCURRENTLY. The org must never end with zero admins; exactly one
/// demote executes and the other is refused at the execution guard.
#[tokio::test]
async fn adv_d33_mutual_demote_never_leaves_org_adminless() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let host = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let unique = db::new_id();

        // THREE rounds, each on a FRESH org with an exact 2-admin bench
        // (round-2 hardening): the FOR UPDATE serialization must hold
        // every time the two transactions genuinely overlap — one lucky
        // trial proves little. A shared org would grow the bench to 3,
        // where BOTH cross-demotes are legal and (1,1) is unprovable.
        for round in 0..3 {
            let (_, org) = call(
                &app,
                api(
                    "POST",
                    "/api/v1/orgs",
                    &host,
                    Some(&json!({
                        "name": "D",
                        "slug": format!("mut-{unique}-r{round}")
                    })),
                ),
            )
            .await;
            let org_id = org["id"].as_str().unwrap().to_string();
            let mut ids = Vec::new();
            let mut emails = Vec::new();
            for who in ["a", "b"] {
                let email = format!("mut-{who}-{unique}-r{round}@t.dev");
                let (_, m) = call(
                    &app,
                    api(
                        "POST",
                        "/api/v1/members",
                        &host,
                        Some(&json!({
                            "org_id": org_id, "email": email,
                            "password": "password123", "role": "org_admin"
                        })),
                    ),
                )
                .await;
                ids.push(m["id"].as_str().unwrap().to_string());
                emails.push(email);
            }
            let ta = member_login(&app, &emails[0]).await;
            let tb = member_login(&app, &emails[1]).await;

            // Cross requests: A demotes B, B demotes A — both legal rows.
            let (st, p1) = call(
                &app,
                api(
                    "POST",
                    &format!("/api/v1/orgs/{org_id}/admins/{}/demote", ids[1]),
                    &ta,
                    None,
                ),
            )
            .await;
            assert_eq!(st, StatusCode::ACCEPTED);
            let (st, p2) = call(
                &app,
                api(
                    "POST",
                    &format!("/api/v1/orgs/{org_id}/admins/{}/demote", ids[0]),
                    &tb,
                    None,
                ),
            )
            .await;
            assert_eq!(st, StatusCode::ACCEPTED);
            let d1 = p1["id"].as_str().unwrap().to_string();
            let d2 = p2["id"].as_str().unwrap().to_string();

            // Concurrent cross-approvals — the F3 attack window.
            let app1 = app.clone();
            let app2 = app.clone();
            let tb2 = tb.clone();
            let ta2 = ta.clone();
            let u1 = format!("/api/v1/orgs/{org_id}/pending-actions/{d1}/approve");
            let u2 = format!("/api/v1/orgs/{org_id}/pending-actions/{d2}/approve");
            let _ = tokio::join!(
                async move { call(&app1, api("POST", &u1, &tb2, None)).await },
                async move { call(&app2, api("POST", &u2, &ta2, None)).await },
            );

            // DB read in ASSERTION only (harness rule): never zero admins.
            let admins = tributary_control::db::org_admin_count(&test_db.pool, &org_id)
                .await
                .unwrap();
            assert!(
                admins >= 1,
                "F3 TOCTOU (round {round}): mutual demotes left {admins} admins"
            );

            // Exactly one demote executed; the other was refused at
            // execution. Read through the HOST: one of A/B just lost
            // admin status in the race, and which one varies per run.
            let mut executed = 0;
            let mut rejected = 0;
            for action in [&d1, &d2] {
                let (st, rows) = call(
                    &app,
                    api(
                        "GET",
                        &format!("/api/v1/orgs/{org_id}/pending-actions?status=executed"),
                        &host,
                        None,
                    ),
                )
                .await;
                assert_eq!(st, StatusCode::OK, "list executed: {rows}");
                if rows
                    .as_array()
                    .expect("executed list is an array")
                    .iter()
                    .any(|p| p["id"] == json!(action))
                {
                    executed += 1;
                }
                let (st, rows) = call(
                    &app,
                    api(
                        "GET",
                        &format!("/api/v1/orgs/{org_id}/pending-actions?status=rejected"),
                        &host,
                        None,
                    ),
                )
                .await;
                assert_eq!(st, StatusCode::OK, "list rejected: {rows}");
                if rows
                    .as_array()
                    .expect("rejected list is an array")
                    .iter()
                    .any(|p| p["id"] == json!(action))
                {
                    rejected += 1;
                }
            }
            assert_eq!(
                (executed, rejected),
                (1, 1),
                "round {round}: one demote lands, the other hits the last-admin guard"
            );
        }
    }
}

/// ROTATION (final-scan 2.4 + D33 gate fix): rotation must carry the IP
/// allowlist on BOTH paths — the routine direct one (member key: the new
/// credential stays pinned) and the deferred one (another admin's key:
/// defers with no key material disclosed, the approver receives the
/// reveal-once raw, and the reissued key stays pinned).
#[tokio::test]
async fn adv_rotation_gated_and_carries_allowlist() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let host = bootstrap_admin_and_login(&test_db.pool, &app).await;

        // -- LEG 1: routine rotation carries the pin (member key). ------
        let stack = provision_stack(&app, &host, &mock).await;
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/allowlists",
                    stack.pool_id, stack.key_id
                ),
                &host,
                Some(&json!({"cidr": "10.0.0.0/8"})),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let before = mock.captured_count();
        let (st, rotated) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/rotate",
                    stack.pool_id, stack.key_id
                ),
                &host,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED, "member key: routine rotation");
        let new_raw = rotated["raw"].as_str().unwrap().to_string();
        assert_eq!(
            mock.captured_count(),
            before,
            "rotation touches no upstream"
        );

        // OLD credential dead before upstream.
        let (st, _) = call(&app, chat_req(&stack.key_raw, None, &chat_body())).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        // NEW credential from OUTSIDE the pin → 403, no upstream — the
        // CIDR pin survived the rotation (2.4).
        let before = mock.captured_count();
        let (st, _) = call(&app, chat_req(&new_raw, Some("203.0.113.7"), &chat_body())).await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "rotation must carry the IP allowlist"
        );
        assert_eq!(mock.captured_count(), before);
        // In-range on the new credential → served.
        let (st, _) = call(&app, chat_req(&new_raw, Some("10.42.0.1"), &chat_body())).await;
        assert_eq!(st, StatusCode::OK);

        // -- LEG 2: another admin's key → DEFERS; approver gets the raw.
        let unique = db::new_id();
        let (_, org) = call(
            &app,
            api(
                "POST",
                "/api/v1/orgs",
                &host,
                Some(&json!({"name": "RO", "slug": format!("rot-{unique}")})),
            ),
        )
        .await;
        let org_id = org["id"].as_str().unwrap().to_string();
        let mut ids = Vec::new();
        let mut emails = Vec::new();
        for who in ["a", "b"] {
            let email = format!("rot-{who}-{unique}@t.dev");
            let (_, m) = call(
                &app,
                api(
                    "POST",
                    "/api/v1/members",
                    &host,
                    Some(&json!({
                        "org_id": org_id, "email": email,
                        "password": "password123", "role": "org_admin"
                    })),
                ),
            )
            .await;
            ids.push(m["id"].as_str().unwrap().to_string());
            emails.push(email);
        }
        let ta = member_login(&app, &emails[0]).await;
        let tb = member_login(&app, &emails[1]).await;
        let (_, pool) = call(
            &app,
            api(
                "POST",
                "/api/v1/pools",
                &ta,
                Some(&json!({
                    "org_id": org_id, "name": "rp2", "model_allowlist": ["m-test"]
                })),
            ),
        )
        .await;
        let pool_id = pool["id"].as_str().unwrap().to_string();
        // The pool needs a provider key or every chat 503s at upstream
        // selection (mirror provision_stack's provider step).
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/provider-keys"),
                &ta,
                Some(&json!({"key": TEST_UPSTREAM_KEY, "base_url": mock.base_url})),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let (_, bkey) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys"),
                &ta,
                Some(&json!({"user_id": ids[1]})),
            ),
        )
        .await;
        let bkey_id = bkey["id"].as_str().unwrap().to_string();
        let bkey_raw = bkey["raw"].as_str().unwrap().to_string();
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys/{bkey_id}/allowlists"),
                &ta,
                Some(&json!({"cidr": "10.0.0.0/8"})),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);

        // A rotating B's key: DEFERS, nothing disclosed, nothing executed.
        let (st, pend) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool_id}/keys/{bkey_id}/rotate"),
                &ta,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::ACCEPTED, "rotation of an admin key defers");
        assert_eq!(pend["kind"], "admin_key_rotate");
        assert!(
            pend.get("raw").is_none() && !pend.to_string().contains(&bkey_raw),
            "a deferred action discloses no key material"
        );
        let (st, _) = call(&app, chat_req(&bkey_raw, Some("10.42.0.1"), &chat_body())).await;
        assert_eq!(st, StatusCode::OK, "B's working key survived the request");

        // B (the owner, a distinct admin from the requester) approves and
        // receives the reveal-once raw.
        let action = pend["id"].as_str().unwrap().to_string();
        let (st, decided) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/pending-actions/{action}/approve"),
                &tb,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(decided["action"]["status"], "executed");
        let issued_raw = decided["issued"]["raw"]
            .as_str()
            .expect("approver receives the reveal-once raw")
            .to_string();
        let issued_id = decided["issued"]["key_id"].as_str().unwrap().to_string();
        assert_ne!(issued_raw, bkey_raw);

        // Old credential dead; new credential STAYS PINNED (deferred path
        // copies the allowlist too).
        let (st, _) = call(&app, chat_req(&bkey_raw, Some("10.42.0.1"), &chat_body())).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "old credential is dead");
        let before = mock.captured_count();
        let (st, _) = call(
            &app,
            chat_req(&issued_raw, Some("203.0.113.7"), &chat_body()),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "deferred rotation keeps the CIDR pin"
        );
        assert_eq!(mock.captured_count(), before);
        let (st, _) = call(&app, chat_req(&issued_raw, Some("10.42.0.1"), &chat_body())).await;
        assert_eq!(st, StatusCode::OK);
        // Management view: old rotated, new active.
        let (_, keys) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{pool_id}/keys/manage"),
                &ta,
                None,
            ),
        )
        .await;
        let status_of = |id: &str| {
            keys.as_array()
                .unwrap()
                .iter()
                .find(|k| k["id"] == json!(id))
                .unwrap()["status"]
                .clone()
        };
        assert_eq!(status_of(&bkey_id), json!("rotated"));
        assert_eq!(status_of(&issued_id), json!("active"));

        // -- Reveal confinement negatives (round-2 gap): the raw exists
        //    NOWHERE except the approver's single HTTP response. ------
        let (_, audit_rows) = call(&app, api("GET", "/api/v1/audit?limit=300", &host, None)).await;
        let audit_surface = audit_rows.to_string();
        assert!(
            !audit_surface.contains(&issued_raw),
            "raw key never reaches the audit trail"
        );
        assert!(
            !audit_surface.contains("ciphertext") && !audit_surface.contains("dek_wrapped"),
            "sealed-blob field names never reach the audit trail"
        );
        let (_, exec_rows) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org_id}/pending-actions?status=executed"),
                &host,
                None,
            ),
        )
        .await;
        assert!(
            !exec_rows.to_string().contains(&issued_raw),
            "the executed list view carries no raw"
        );
        let payload_sql = test_db
            .pool
            .sql("SELECT payload_json FROM pending_actions WHERE id = ?");
        let payload: String = sqlx::query(sqlx::AssertSqlSafe(payload_sql.as_str()))
            .bind(&action)
            .fetch_one(test_db.pool.pool())
            .await
            .expect("pending row persists")
            .try_get("payload_json")
            .expect("payload column");
        assert!(
            !payload.contains(&issued_raw),
            "the stored payload carries no raw (it never contained one)"
        );

        // -- Direct refusal (F3 regression): rotating an ALREADY-DEAD key
        //    mints NOTHING and says so. stack.key_id rotated in LEG 1. --
        let (_, keys_before) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{}/keys/manage", stack.pool_id),
                &host,
                None,
            ),
        )
        .await;
        let count_before = keys_before.as_array().expect("manage array").len();
        let (st, err) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/rotate",
                    stack.pool_id, stack.key_id
                ),
                &host,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT, "dead keys do not rotate");
        assert!(
            err["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("only active or suspended keys rotate")),
            "the precheck refuses dead keys with its own message: {err}"
        );
        let (_, keys_after) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{}/keys/manage", stack.pool_id),
                &host,
                None,
            ),
        )
        .await;
        assert_eq!(
            keys_after.as_array().expect("manage array").len(),
            count_before,
            "a refused rotation mints nothing"
        );

        // -- Deferred refusal (F1 regression): the target dies between
        //    request and approval — the approval is REJECTED and no
        //    orphan key row survives behind it. -----------------------
        // B self-rotates the reissued key (own admin key, routine direct).
        let (st, r2) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/keys/{}/rotate", pool_id, issued_id),
                &tb,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED, "B self-rotates routinely");
        let b2_id = r2["id"].as_str().unwrap().to_string();
        // A requests rotating B's (new) key → defers.
        let (st, p3) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/keys/{}/rotate", pool_id, b2_id),
                &ta,
                None,
            ),
        )
        .await;
        assert_eq!(st, StatusCode::ACCEPTED);
        let action3 = p3["id"].as_str().unwrap().to_string();
        // B kills their own key BEFORE approval: self-revoke via the
        // member self-service surface.
        let (st, _) = call(
            &app,
            api("DELETE", &format!("/api/v1/me/keys/{b2_id}"), &tb, None),
        )
        .await;
        assert_eq!(st, StatusCode::NO_CONTENT, "B revokes own key");
        // Host approves → the guard refuses (target 'revoked').
        let (_, keys_pre) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{pool_id}/keys/manage"),
                &host,
                None,
            ),
        )
        .await;
        let pre = keys_pre.as_array().expect("manage array").len();
        let (st, decided3) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{org_id}/pending-actions/{action3}/approve"),
                &host,
                None,
            ),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "approval refuses when the target died: {decided3}"
        );
        let (_, keys_post) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{pool_id}/keys/manage"),
                &host,
                None,
            ),
        )
        .await;
        assert_eq!(
            keys_post.as_array().expect("manage array").len(),
            pre,
            "the refused approval leaves NO orphan key row"
        );
        let (_, rejected_rows) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/orgs/{org_id}/pending-actions?status=rejected"),
                &host,
                None,
            ),
        )
        .await;
        assert!(
            rejected_rows
                .as_array()
                .expect("rejected array")
                .iter()
                .any(|r| r["id"] == json!(action3)),
            "the refused action is recorded as rejected"
        );
    }
}

/// LIVE AUTO-BLOCK SPARES ON-PURPOSE (classifier review j10): a threshold
/// inversion (block everything) would ship green through the existing
/// suite — nothing proved a LIVE org's above-threshold request survives.
/// The mock hit counter is the determinism barrier: scoring provably ran
/// before the zero-flag assert.
#[tokio::test]
async fn adv_live_auto_block_spares_on_purpose() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let classifier = spawn_classifier().await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let host = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &host, &mock).await;

        // Solo org: configure + go-live execute immediately (auto-pass).
        let (st, cfg) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &host,
                Some(&json!({
                    "base_url": classifier.base_url, "dialect": "custom",
                    "model": "jev-align-1", "key": "sk-jev-adv-key-123456",
                    "purpose_statement": "Coding assistance for our team.",
                    "mode": "auto_block", "acknowledge_auto_block": true,
                })),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        assert_eq!(cfg["auto_block_live"], false, "shadow first");
        let (st, cfg) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{}/classifier/go-live", stack.org_id),
                &host,
                Some(&json!({})),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(cfg["auto_block_live"], true);

        let probe_hits = *classifier.hits.lock().unwrap();
        let on_purpose = json!({
            "model": "m-test",
            "messages": [{"role": "user",
                "content": "please refactor this rust function for clarity"}],
        });
        for n in 0..3 {
            let (st, _) = call(&app, chat_req(&stack.key_raw, None, &on_purpose)).await;
            assert_eq!(st, StatusCode::OK, "on-purpose #{n} must pass while LIVE");
        }
        wait_for("all on-purpose requests were scored", || {
            let hits = classifier.hits.clone();
            async move { *hits.lock().unwrap() >= probe_hits + 3 }
        })
        .await;

        // Earned negative: no auto-block flag, key never suspended.
        let (_, flags) = call(
            &app,
            api(
                "GET",
                "/api/v1/review-flags?status=open&limit=100",
                &host,
                None,
            ),
        )
        .await;
        let auto_blocks = flags
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| {
                r["kind"] == "classifier_auto_block" && r["pool_key_id"] == json!(stack.key_id)
            })
            .count();
        assert_eq!(
            auto_blocks, 0,
            "live auto-block must NEVER fire above the block threshold"
        );
        let (st, _) = call(&app, chat_req(&stack.key_raw, None, &on_purpose)).await;
        assert_eq!(st, StatusCode::OK, "key never suspended by on-purpose use");
        // Determinism barrier (round-2): the 3rd scoring decision lands
        // asynchronously — re-read the queue AFTER the final chat so a
        // late block can no longer slip past the earlier zero-read.
        wait_for("third decision settled", || {
            let hits = classifier.hits.clone();
            async move { *hits.lock().unwrap() >= probe_hits + 4 }
        })
        .await;
        let (_, flags2) = call(
            &app,
            api(
                "GET",
                "/api/v1/review-flags?status=open&limit=100",
                &host,
                None,
            ),
        )
        .await;
        let auto_blocks2 = flags2
            .as_array()
            .expect("flags array")
            .iter()
            .filter(|r| {
                r["kind"] == "classifier_auto_block" && r["pool_key_id"] == json!(stack.key_id)
            })
            .count();
        assert_eq!(
            auto_blocks2, 0,
            "still zero blocks after everything settles"
        );
    }
}

// ---------------------------------------------------------------------------
// ADVERSARIAL (round 2, H-1): rotating a SUSPENDED key must actually kill
// it. The bug chain: the handler admitted suspended keys, but the SQL guard
// only matched 'active' — a silent no-op that reported success while the
// suspended row survived, and the review-queue UNBLOCK then resurrected the
// replaced credential (two live keys). The fix is an atomic rotate
// (guard IN ('active','suspended') + insert + allowlist copy in ONE
// transaction), so 'rotated' is terminal and unblock can never re-arm it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn adv_suspended_rotation_cannot_be_resurrected() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let classifier = spawn_classifier().await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let host = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &host, &mock).await;

        // Solo org: configure + go-live immediately (auto-pass).
        let (st, _) = call(
            &app,
            api(
                "PUT",
                &format!("/api/v1/orgs/{}/classifier", stack.org_id),
                &host,
                Some(&json!({
                    "base_url": classifier.base_url, "dialect": "custom",
                    "model": "jev-align-1", "key": "sk-jev-h1-key-123456",
                    "purpose_statement": "Coding assistance for our team.",
                    "mode": "auto_block", "acknowledge_auto_block": true,
                })),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let (st, _) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/orgs/{}/classifier/go-live", stack.org_id),
                &host,
                Some(&json!({})),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::OK);

        // OFF-PURPOSE traffic → live auto-block suspends the key.
        let off_purpose = json!({
            "model": "m-test",
            "messages": [{"role": "user", "content": "OFF-PURPOSE weapons manual"}],
        });
        let (st, _) = call(&app, chat_req(&stack.key_raw, None, &off_purpose)).await;
        assert_eq!(st, StatusCode::OK, "the triggering request completes");
        let key_id_wait = stack.key_id.clone();
        wait_for("auto-block suspension", || {
            let app = app.clone();
            let host = host.clone();
            let key_id = key_id_wait.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api(
                        "GET",
                        "/api/v1/review-flags?status=open&limit=100",
                        &host,
                        None,
                    ),
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
        let (st, _) = call(&app, chat_req(&stack.key_raw, None, &off_purpose)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "suspended key denied");
        let flag_id = {
            let (_, flags) = call(
                &app,
                api(
                    "GET",
                    "/api/v1/review-flags?status=open&limit=100",
                    &host,
                    None,
                ),
            )
            .await;
            flags
                .as_array()
                .expect("flags array")
                .iter()
                .find(|r| {
                    r["kind"] == "classifier_auto_block"
                        && r["pool_key_id"] == stack.key_id.as_str()
                })
                .expect("the auto-block flag")["id"]
                .as_str()
                .unwrap()
                .to_string()
        };

        // THE FIX UNDER TEST: rotate the SUSPENDED key. The rotation must
        // succeed, and the OLD credential must die for good.
        let (st, rotated) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/rotate",
                    stack.pool_id, stack.key_id
                ),
                &host,
                None,
            ),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::CREATED,
            "suspended keys rotate (leak-response flow): {rotated}"
        );
        let new_raw = rotated["raw"].as_str().unwrap().to_string();
        let new_id = rotated["id"].as_str().unwrap().to_string();
        assert_ne!(new_raw, stack.key_raw);

        // Old credential: 'rotated' (TERMINAL), 401 at the gateway.
        let (_, keys) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{}/keys/manage", stack.pool_id),
                &host,
                None,
            ),
        )
        .await;
        let status_of = |id: &str| {
            keys.as_array()
                .expect("manage array")
                .iter()
                .find(|k| k["id"] == json!(id))
                .unwrap()["status"]
                .clone()
        };
        assert_eq!(
            status_of(&stack.key_id),
            json!("rotated"),
            "rotated is terminal"
        );
        assert_eq!(status_of(&new_id), json!("active"));
        let (st, _) = call(&app, chat_req(&stack.key_raw, None, &off_purpose)).await;
        assert_eq!(
            st,
            StatusCode::UNAUTHORIZED,
            "the replaced credential is dead"
        );

        // The replacement is CLEAN: on-purpose traffic serves immediately
        // (the containment hold belonged to the old key — rotation is the
        // documented leak-response resolution, and it is audited).
        let on_purpose = json!({
            "model": "m-test",
            "messages": [{"role": "user",
                "content": "please refactor this rust function for clarity"}],
        });
        let (st, _) = call(&app, chat_req(&new_raw, None, &on_purpose)).await;
        assert_eq!(st, StatusCode::OK, "the replacement key works");

        // NO RESURRECTION: the flag is still open, but unblock can only
        // lift 'suspended' — the old row is 'rotated' and must 409.
        let (st, err) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/review-flags/{flag_id}/resolve"),
                &host,
                Some(&json!({"action": "unblock"})),
            ),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::CONFLICT,
            "unblock must NOT resurrect a rotated key: {err}"
        );
        let (st, _) = call(&app, chat_req(&stack.key_raw, None, &off_purpose)).await;
        assert_eq!(
            st,
            StatusCode::UNAUTHORIZED,
            "still dead after the unblock attempt — exactly ONE live key"
        );
    }
}
