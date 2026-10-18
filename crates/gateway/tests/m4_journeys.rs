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
use sqlx::Row; // trait for AnyRow::get in assertion helpers
use tower::ServiceExt;
use tributary_gateway::db;
use tributary_gateway::keys;

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
    member_id: String,
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
        member_id,
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

/// Counts rows matching one bind in a table (ASSERTION-only helper).
async fn count_rows(pool: &db::Db, table: &str, column: &str, value: &str) -> i64 {
    let sql = pool.sql(&format!(
        "SELECT COUNT(*) AS n FROM {table} WHERE {column} = ?"
    ));
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(value)
        .fetch_one(pool.pool())
        .await
        .expect("count query");
    row.get::<i64, _>("n")
}

// ---------------------------------------------------------------------------
// J1 — leak demo: honey key planted via API, used, detected, capped
// ---------------------------------------------------------------------------

#[tokio::test]
async fn j1_honey_key_journey() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // -- Plant a honey key through the admin API (raw shown ONCE). -----
        let (_, planted) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/canary-keys", stack.pool_id),
                &token,
                Some(&json!({"count": 1})),
            ),
        )
        .await;
        let honey_raw = planted["keys"][0]["raw"].as_str().unwrap().to_string();
        let honey_prefix = planted["keys"][0]["key_prefix"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!honey_raw.is_empty());

        // -- The shared listing carries it shape-identically (directive). --
        let (_, listing) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{}/keys", stack.pool_id),
                &token,
                None,
            ),
        )
        .await;
        let rows = listing.as_array().expect("listing array");
        assert_eq!(rows.len(), 2, "one live + one canary");
        for row in rows {
            let mut keys: Vec<&str> = row
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort();
            assert_eq!(
                keys,
                vec!["created_at", "id", "key_prefix", "pool_id", "status"],
                "shared listing rows are exactly the uniform five fields"
            );
        }
        let listed_canary = rows
            .iter()
            .find(|r| r["key_prefix"].as_str() == Some(&honey_prefix))
            .expect("honey row is present in the shared listing");
        assert_eq!(listed_canary["status"], "active");
        // Ordering tell: rows come back in created_at order (canaries are
        // backdated into that same ordering at plant time).
        let times: Vec<i64> = rows
            .iter()
            .map(|r| r["created_at"].as_i64().unwrap())
            .collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted, "listing is ordered by created_at");

        // -- Use the honey key at the gateway: generic 401, byte-identical. -
        let before_upstream = mock.captured_count();
        let (honey_status, honey_bytes) = call_raw(
            &app,
            chat_req(&honey_raw, Some("203.0.113.9"), &chat_body()),
        )
        .await;
        assert_eq!(honey_status, StatusCode::UNAUTHORIZED);
        // The control case: a well-formed key that matches NO row.
        let unknown = keys::generate_key().raw;
        let (unknown_status, unknown_bytes) =
            call_raw(&app, chat_req(&unknown, Some("203.0.113.9"), &chat_body())).await;
        assert_eq!(unknown_status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            honey_bytes, unknown_bytes,
            "honey-key 401 is byte-identical to the unknown-key 401"
        );

        // -- Capped: it never served traffic (no upstream call, no usage). --
        assert_eq!(
            mock.captured_count(),
            before_upstream,
            "honey key never reaches upstream"
        );
        let usage =
            db::usage_events_for_key(&test_db.pool, planted["keys"][0]["id"].as_str().unwrap())
                .await
                .expect("usage read");
        assert!(
            usage.is_empty(),
            "no usage row is ever written for a canary"
        );

        // -- Review flag lands in the admin queue with the masked prefix. --
        let honey_prefix_ref = honey_prefix.clone();
        wait_for("honey_key_used review flag", || {
            let app = app.clone();
            let token = token.clone();
            let honey_prefix = honey_prefix_ref.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "honey_key_used"
                            && r["detail_json"]["key_prefix"] == honey_prefix.as_str()
                    })
                })
            }
        })
        .await;

        // -- Email alert: outbox row exists (log-fallback path — SMTP is --
        // unconfigured in tests; the founder's manual journey proves the
        // SMTP leg).
        wait_for("honey-key email alert row", || {
            let pool = test_db.pool.clone();
            let org = stack.org_id.clone();
            async move { count_rows(&pool, "email_alerts", "org_id", &org).await > 0 }
        })
        .await;

        // -- Audit row via the public audit API. ----------------------------
        wait_for("security.honey_key_used audit row", || {
            let app = app.clone();
            let token = token.clone();
            async move {
                let (_, audit) =
                    call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
                audit.as_array().is_some_and(|rows| {
                    rows.iter()
                        .any(|r| r["action"] == "security.honey_key_used")
                })
            }
        })
        .await;
    }
}

// ---------------------------------------------------------------------------
// Directive-1 test: byte-shape indistinguishability of the shared listing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shared_listing_shape_identity_across_kinds() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // A second live key + three canaries: a scraper's-eye mix. The
        // 60 ms gap between the two live keys dominates the canaries'
        // backdate window [pool.created_at, plant-time], making
        // "at least one live row after a canary" deterministic enough
        // (P(all 3 canaries inside the final few ms) ≈ 0).
        call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/keys", stack.pool_id),
                &token,
                Some(&json!({"user_id": stack.member_id})),
            ),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/keys", stack.pool_id),
                &token,
                Some(&json!({"user_id": stack.member_id})),
            ),
        )
        .await;
        call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/canary-keys", stack.pool_id),
                &token,
                Some(&json!({"count": 3})),
            ),
        )
        .await;

        // The admin management view tells US (the test) which ids are canary.
        let (_, canaries) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{}/canaries", stack.pool_id),
                &token,
                None,
            ),
        )
        .await;
        let canary_ids: Vec<&str> = canaries["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k["id"].as_str().unwrap())
            .collect();
        assert_eq!(canary_ids.len(), 3, "management view reveals canaries");

        let (_, listing) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{}/keys", stack.pool_id),
                &token,
                None,
            ),
        )
        .await;
        let rows = listing.as_array().unwrap();
        assert_eq!(rows.len(), 6, "three live + three canary in one listing");

        // (a) Same field set on every row — no kind marker, no user_id.
        let mut reference: Option<std::collections::BTreeSet<&str>> = None;
        for row in rows {
            let set: std::collections::BTreeSet<&str> = row
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert!(!set.contains("kind") && !set.contains("user_id"));
            match &reference {
                None => reference = Some(set),
                Some(r) => assert_eq!(&set, r, "every row carries the same fields"),
            }
        }

        // (b) Same VALUE TYPES across kinds per field (a null-valued field
        // on one kind only would be a filterable tell).
        for row in rows {
            for (field, value) in row.as_object().unwrap() {
                let expected_str =
                    matches!(field.as_str(), "id" | "pool_id" | "key_prefix" | "status");
                if expected_str {
                    assert!(value.is_string(), "{field} must be a string on every row");
                } else {
                    assert!(value.is_i64(), "{field} must be a number on every row");
                }
            }
        }

        // (c) Same ordering key for both kinds (created_at, ascending) —
        // canaries interleave via backdating rather than clustering.
        let times: Vec<i64> = rows
            .iter()
            .map(|r| r["created_at"].as_i64().unwrap())
            .collect();
        let mut sorted_times = times.clone();
        sorted_times.sort();
        assert_eq!(times, sorted_times);
        // And the canaries are not all-at-the-end: at least one live row
        // comes AFTER a canary row (backdating made them swim).
        let canary_positions: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| canary_ids.contains(&r["id"].as_str().unwrap_or_default()))
            .map(|(i, _)| i)
            .collect();
        let last_canary = *canary_positions.last().unwrap();
        assert!(
            last_canary < rows.len() - 1,
            "canaries must interleave, not cluster at the end of the listing"
        );
    }
}

// ---------------------------------------------------------------------------
// J2 — canary model alias journey
// ---------------------------------------------------------------------------

#[tokio::test]
async fn j2_canary_model_journey() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // Plant a per-member canary alias (SPEC §9 "per-member aliases").
        let alias = format!("gpt-4o-mini-{}", &db::new_id()[..6]);
        let (_, planted) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{}/canary-models", stack.pool_id),
                &token,
                Some(&json!({"alias": alias, "user_id": stack.member_id})),
            ),
        )
        .await;
        assert_eq!(planted["alias"], alias.as_str());

        // The member's /v1/models lists it — shaped exactly like the reals.
        let (_, models) = call(&app, api("GET", "/v1/models", &stack.key_raw, None)).await;
        let listed = models["data"].as_array().unwrap();
        let entry = listed
            .iter()
            .find(|m| m["id"] == alias.as_str())
            .expect("canary alias appears in the member's models");
        for real in listed.iter().filter(|m| m["id"] == "m-test") {
            assert_eq!(
                entry.as_object().unwrap().keys().len(),
                real.as_object().unwrap().keys().len()
            );
        }

        // Request targets it → denied LOCALLY with the allowlist message.
        let before = mock.captured_count();
        let body = json!({"model": alias, "messages": [{"role": "user", "content": "hi"}]});
        let (status, err) = call(&app, chat_req(&stack.key_raw, None, &body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            err["error"]["message"],
            format!("Model '{alias}' is not allowed for this pool."),
            "canary deny uses the same message as any allowlist reject"
        );
        assert_eq!(mock.captured_count(), before, "upstream never sees it");

        // Same denial shape as a genuinely-disallowed model (envelope).
        let other =
            json!({"model": "not-a-real-model", "messages": [{"role": "user", "content": "hi"}]});
        let (other_status, other_err) = call(&app, chat_req(&stack.key_raw, None, &other)).await;
        assert_eq!(other_status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"]["type"], other_err["error"]["type"]);
        assert_eq!(err["error"]["code"], other_err["error"]["code"]);

        // Detection: flag + audit (email asserted through the outbox).
        let alias_ref = alias.clone();
        wait_for("canary_model review flag", || {
            let app = app.clone();
            let token = token.clone();
            let alias = alias_ref.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter().any(|r| {
                        r["kind"] == "canary_model" && r["detail_json"]["model"] == alias.as_str()
                    })
                })
            }
        })
        .await;
        wait_for("canary email alert", || {
            let pool = test_db.pool.clone();
            let org = stack.org_id.clone();
            async move { count_rows(&pool, "email_alerts", "org_id", &org).await > 0 }
        })
        .await;
        wait_for("security.canary_model_hit audit row", || {
            let app = app.clone();
            let token = token.clone();
            async move {
                let (_, audit) =
                    call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
                audit.as_array().is_some_and(|rows| {
                    rows.iter()
                        .any(|r| r["action"] == "security.canary_model_hit")
                })
            }
        })
        .await;
    }
}

// ---------------------------------------------------------------------------
// J3 — classifier journey: alert mode → shadow auto-block → live → unblock
// ---------------------------------------------------------------------------

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
