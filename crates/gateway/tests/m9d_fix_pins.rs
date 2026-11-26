//! M9-D FIX PINS — Session D (doctrine-suite step 14, fix session).
//!
//! Session C's PoCs were assert-the-EXPLOIT standing pins: each flipped
//! red the day its finding was fixed (they are red NOW, by design — the
//! exploits are dead). These tests are their PERMANENT successors: each
//! pins the HONEST post-fix state of one finding, so the fix can never
//! silently regress. Additive only — no existing test was touched.
//!
//! Journey proof (workflow step 12): everything rides the PUBLIC API
//! (pools, deposits, member keys, chat, settle); the ledger is read
//! through its public trait exactly as Session C's PoCs did (the m5
//! sanction: raw SQL solely as assertion-only reads).
//!
//! Pin → finding map:
//! * settle_retry_replays_the_first_run_exactly … (A-048)
//! * settle_posts_the_books_and_positions_are_caches … (A-049)
//! * claims_mirror_unburned_principal_and_capacity_mirrors_gross … (A-049)
//! * an_adjacent_window_settles_only_new_burns … (A-048 delta attribution)
//! * a_zero_usage_report_burns_the_reserve_estimate … (A-053 leg 2)
//! * a_refused_pick_falls_back_to_sibling_capacity … (A-052)
//! * a_hyphen_prefix_declarer_serves_the_longer_model … (A-050 leg 2)
//! * burn_clamps_at_the_contribution_and_overage_never_distributes … (A-047)

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::IntoResponse;
use common::{MockMode, MockUpstream, TEST_UPSTREAM_KEY, spawn_mock, test_app, test_dbs};
use serde_json::{Value, json};
use tower::ServiceExt;
use tributary_gateway::db;
use tributary_ledger::chart::{AccountCode, OwnerKind, PLATFORM_OWNER_ID};
use tributary_ledger::{Ledger, SqlLedger};

// The m9c harness shapes, mirrored locally (this file owns its rig):
// request builders, the merged-app call, and the public-API fixtures.

/// An authenticated JSON request against the control plane.
fn api(method: &str, uri: &str, token: &str, body: Option<&Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    match body {
        Some(body) => builder.body(Body::from(body.to_string())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

/// Fires one request at the merged app; returns (status, parsed body).
async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(req).await.expect("oneshot");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// A chat request on the data plane with the member's raw key.
fn chat_req(key: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Host admin bootstrap + login (the product's own bootstrap path).
async fn bootstrap_admin_and_login(pool: &db::Db, app: &axum::Router) -> String {
    let _ =
        tributary_control::create_host_admin(pool, "root@tributary.dev", "hunter2hunter2").await;
    let (status, body) = call(
        app,
        api(
            "POST",
            "/api/v1/auth/login",
            "",
            Some(&json!({"email": "root@tributary.dev", "password": "hunter2hunter2"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "host login: {body}");
    body["token"].as_str().unwrap().to_string()
}

/// A fresh org + ROOT pool through the public API; returns (org, pool).
async fn org_and_root_pool(app: &axum::Router, token: &str, models: &[&str]) -> (String, String) {
    let (status, org) = call(
        app,
        api(
            "POST",
            "/api/v1/orgs",
            token,
            Some(&json!({"name": "M9D Fix Org", "slug": format!("m9d-{}", db::new_id())})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "org: {org}");
    let org = org["id"].as_str().unwrap().to_string();
    let (status, pool) = call(
        app,
        api(
            "POST",
            "/api/v1/pools",
            token,
            Some(&json!({
                "org_id": org,
                "name": "Fix Pin Pool",
                "kind": "root",
                "model_allowlist": models,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "root pool: {pool}");
    (org, pool["id"].as_str().unwrap().to_string())
}

/// A deposit through the M9 contract path; returns (depositor_id, key_id).
async fn deposit(
    app: &axum::Router,
    token: &str,
    pool_id: &str,
    provider: &str,
    family: &str,
    contributed_micros: i64,
    base_url: &str,
) -> (String, String) {
    let (status, resp) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/deposits"),
            token,
            Some(&json!({
                "provider": provider,
                "key": TEST_UPSTREAM_KEY,
                "base_url": base_url,
                "model_families": [family],
                "contributed_micros": contributed_micros,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "deposit: {resp}");
    (
        resp["depositor_id"].as_str().unwrap().to_string(),
        resp["provider_key_id"].as_str().unwrap().to_string(),
    )
}

/// A member + pool key on the pool (existing V1 public path).
async fn member_key(app: &axum::Router, token: &str, org_id: &str, pool_id: &str) -> String {
    let email = format!("m9d-{}@trib.test", db::new_id());
    let (status, member) = call(
        app,
        api(
            "POST",
            "/api/v1/members",
            token,
            Some(&json!({"org_id": org_id, "email": email, "password": "hunter2hunter2"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "member: {member}");
    let (status, key) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/keys"),
            token,
            Some(&json!({"user_id": member["id"].as_str().unwrap()})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "key: {key}");
    key["raw"].as_str().unwrap().to_string()
}

/// POST /settle with an explicit window and fee.
async fn settle(
    app: &axum::Router,
    token: &str,
    pool_id: &str,
    from_ms: i64,
    to_ms: i64,
    fee_bps: i64,
) -> (StatusCode, Value) {
    call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/settle"),
            token,
            Some(&json!({"from_ms": from_ms, "to_ms": to_ms, "fee_bps": fee_bps})),
        ),
    )
    .await
}

/// GET /bucket-positions.
async fn positions(app: &axum::Router, token: &str, pool_id: &str) -> Vec<Value> {
    let (status, body) = call(
        app,
        api(
            "GET",
            &format!("/api/v1/pools/{pool_id}/bucket-positions"),
            token,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bucket-positions: {body}");
    body.as_array().cloned().unwrap_or_default()
}

/// One numeric field summed over a depositor's position rows.
fn pos_sum(rows: &[Value], depositor: &str, field: &str) -> i64 {
    rows.iter()
        .filter(|r| r["depositor_id"] == json!(depositor))
        .map(|r| r[field].as_i64().unwrap_or(0))
        .sum()
}

/// A depositor's payout total from a settle response.
fn settle_total_for(body: &Value, depositor: &str) -> i64 {
    body["payouts"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|p| p["depositor_id"] == json!(depositor))
                .map(|p| p["amount_micros"].as_i64().unwrap_or(0))
                .sum()
        })
        .unwrap_or(0)
}

/// One account's normal-side balance through the PUBLIC ledger trait
/// (0 when the account was never opened — the shape the A-049 PoC used).
async fn normal_balance(
    ledger: &SqlLedger,
    code: AccountCode,
    kind: OwnerKind,
    owner: &str,
) -> i64 {
    match ledger
        .account_id(code, kind, owner)
        .await
        .expect("account id lookup")
    {
        Some(id) => ledger
            .account_balance(&id)
            .await
            .expect("balance read")
            .normal
            .micros(),
        None => 0,
    }
}

/// A mock upstream reporting ATTACKER-CHOSEN token counts (A-053 shapes).
#[allow(clippy::type_complexity)] // mirrors common::spawn_mock's state tuple
async fn spawn_mock_usage(prompt_tokens: i64, completion_tokens: i64) -> MockUpstream {
    let captured: std::sync::Arc<std::sync::Mutex<Vec<common::Captured>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = (prompt_tokens, completion_tokens, captured.clone());
    let app = axum::Router::new()
        .route(
            "/chat/completions",
            axum::routing::post(
                |axum::extract::State(state): axum::extract::State<(
                    i64,
                    i64,
                    std::sync::Arc<std::sync::Mutex<Vec<common::Captured>>>,
                )>,
                 headers: axum::http::HeaderMap,
                 body: axum::body::Bytes| async move {
                    {
                        let mut guard = state.2.lock().expect("mock capture lock");
                        guard.push(common::Captured {
                            auth: headers
                                .get(header::AUTHORIZATION)
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                            body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                        });
                    }
                    axum::Json(json!({
                        "id": "chatcmpl-m9d",
                        "object": "chat.completion",
                        "model": "m-test",
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": "Hello!"},
                            "finish_reason": "stop",
                        }],
                        "usage": {
                            "prompt_tokens": state.0,
                            "completion_tokens": state.1,
                        },
                    }))
                    .into_response()
                },
            ),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock bind");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    MockUpstream {
        base_url: format!("http://{addr}"),
        captured,
    }
}

/// The plain chat body (string content, no declared cap).
fn plain_chat_body(model: &str) -> Value {
    json!({"model": model, "messages": [{"role": "user", "content": "hi"}]})
}

/// A-048: an identical-window settle retry is a NO-OP that replays the
/// first run's response exactly — the payout is computed once, the
/// position is credited once, and the books post once.
#[tokio::test]
async fn settle_retry_replays_the_first_run_exactly_and_never_re_credits() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test"]).await;
        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;
        for _ in 0..3 {
            let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");
        }

        let now = db::now_ms();
        let (s1, b1) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s1, StatusCode::OK, "[{td_name}] settle #1: {b1}");
        assert_eq!(b1["distributable_micros"], json!(8_100));
        let first_payout = settle_total_for(&b1, &dep);

        // THE PIN: the identical retry returns the FIRST result verbatim…
        let (s2, b2) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s2, StatusCode::OK, "[{td_name}] retry accepted: {b2}");
        assert_eq!(
            b2, b1,
            "[{td_name}] the retry must replay the first response byte-for-byte"
        );

        // …and credits nothing a second time.
        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_sum(&rows, &dep, "earned_micros"),
            first_payout,
            "[{td_name}] earned stays at the first run's payout — no double credit"
        );
        let ledger = SqlLedger::new(td.pool.clone());
        let payable = normal_balance(
            &ledger,
            AccountCode::PayableDepositor,
            OwnerKind::Depositor,
            &dep,
        )
        .await;
        assert_eq!(
            payable, first_payout,
            "[{td_name}] the books also hold exactly one payout"
        );
    }
}

/// A-049 (director ruling 2026-11-19): /settle POSTS the books — each
/// depositor's payout lands in payable.depositor, the fee in
/// equity.operator (D39), the burned principal leaves claims.contributed
/// via capacity.consume — and the positions are true caches of exactly
/// those balances (books lead; drift would be a failing state).
#[tokio::test]
async fn settle_posts_the_books_and_positions_are_caches_of_the_balances() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test"]).await;
        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;
        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");

        let now = db::now_ms();
        let (s, b) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s, StatusCode::OK, "[{td_name}] settle: {b}");
        let payout = settle_total_for(&b, &dep);
        let fee = b["fee_micros"].as_i64().unwrap_or(0);
        assert_eq!(payout, 2_484, "[{td_name}] 0.92 × 2,700");
        assert_eq!(fee, 216, "[{td_name}] 0.08 × 2,700");

        let ledger = SqlLedger::new(td.pool.clone());
        let payable = normal_balance(
            &ledger,
            AccountCode::PayableDepositor,
            OwnerKind::Depositor,
            &dep,
        )
        .await;
        assert_eq!(
            payable, payout,
            "[{td_name}] payable.depositor == the depositor's payout"
        );
        // equity.operator is PLATFORM-scoped and accumulates across every
        // settle on a shared test database — the pin is the DELTA this
        // run accrued, not the global balance.
        let equity_after = normal_balance(
            &ledger,
            AccountCode::EquityOperator,
            OwnerKind::Operator,
            PLATFORM_OWNER_ID,
        )
        .await;
        assert!(
            equity_after >= fee,
            "[{td_name}] equity.operator grew by at least this run's fee (got {equity_after})"
        );
        let claims = normal_balance(
            &ledger,
            AccountCode::ClaimsContributed,
            OwnerKind::Depositor,
            &dep,
        )
        .await;
        assert_eq!(
            claims,
            5_000_000 - 2_700,
            "[{td_name}] the burned principal left claims via capacity.consume"
        );
        // solvency_report SEES payables now (rule 7's books are no longer
        // blind to settled depositor earnings) — the global Σ payable on
        // a shared test database, hence the ≥.
        let solvency = ledger.solvency_report().await.expect("solvency report");
        assert!(
            solvency.payable.micros() >= payout,
            "[{td_name}] solvency sees payables ({} ≥ {payout})",
            solvency.payable.micros()
        );

        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_sum(&rows, &dep, "earned_micros"),
            payout,
            "[{td_name}] positions cache the payable"
        );
    }
}

/// A-049 companion: the books mirror the position columns exactly —
/// claims.contributed == Σ (contributed − burned) and
/// capacity.contributed == Σ contributed per depositor — so the two
/// truths cannot drift apart silently.
#[tokio::test]
async fn claims_mirror_unburned_principal_and_capacity_mirrors_gross_contributed() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test", "m-mini"]).await;
        // One depositor, two buckets (the depositor_id link).
        let (dep, _k1) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            3_000_000,
            &mock.base_url,
        )
        .await;
        let (_same, _k2) = deposit_link(
            &app,
            &token,
            &pool,
            "openai",
            "m-mini",
            2_000_000,
            &mock.base_url,
            &dep,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;
        for model in ["m-test", "m-test", "m-mini"] {
            let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body(model))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] {model}: {resp}");
        }

        let now = db::now_ms();
        let (s, b) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s, StatusCode::OK, "[{td_name}] settle: {b}");

        let rows = positions(&app, &token, &pool).await;
        let contributed: i64 = pos_sum(&rows, &dep, "contributed_micros");
        let burned: i64 = pos_sum(&rows, &dep, "burned_micros");
        assert_eq!(
            contributed, 5_000_000,
            "[{td_name}] gross contributed is deposit-stable"
        );
        assert_eq!(burned, 3 * 2_700, "[{td_name}] three metered requests");

        let ledger = SqlLedger::new(td.pool.clone());
        let claims = normal_balance(
            &ledger,
            AccountCode::ClaimsContributed,
            OwnerKind::Depositor,
            &dep,
        )
        .await;
        assert_eq!(
            claims,
            contributed - burned,
            "[{td_name}] claims == unburned principal"
        );
        let capacity = normal_balance(
            &ledger,
            AccountCode::CapacityContributed,
            OwnerKind::Depositor,
            &dep,
        )
        .await;
        assert_eq!(
            capacity, contributed,
            "[{td_name}] capacity == gross contributed"
        );
    }
}

/// A deposit linked to an EXISTING depositor (the contract's
/// `depositor_id` field — one depositor, many buckets).
#[allow(clippy::too_many_arguments)] // test rig: explicit args beat a builder struct (m9c precedent)
async fn deposit_link(
    app: &axum::Router,
    token: &str,
    pool_id: &str,
    provider: &str,
    family: &str,
    contributed_micros: i64,
    base_url: &str,
    depositor: &str,
) -> (String, String) {
    let (status, resp) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/deposits"),
            token,
            Some(&json!({
                "provider": provider,
                "key": TEST_UPSTREAM_KEY,
                "base_url": base_url,
                "model_families": [family],
                "contributed_micros": contributed_micros,
                "depositor_id": depositor,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "linked deposit: {resp}");
    (
        resp["depositor_id"].as_str().unwrap().to_string(),
        resp["provider_key_id"].as_str().unwrap().to_string(),
    )
}

/// A-048 delta attribution: burns that land AFTER a settled window
/// attribute to the NEXT window only — windows never re-batch, even when
/// they overlap.
#[tokio::test]
async fn an_adjacent_window_settles_only_the_new_burns() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test"]).await;
        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;
        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] chat 1: {resp}");

        let now = db::now_ms();
        let (s1, b1) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s1, StatusCode::OK, "[{td_name}] settle 1: {b1}");
        assert_eq!(b1["distributable_micros"], json!(2_700));

        // New traffic, then an OVERLAPPING window that starts at 0 again:
        // only the new burn attributes.
        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] chat 2: {resp}");
        let now2 = db::now_ms();
        let (s2, b2) = settle(&app, &token, &pool, 0, now2 + 60_000, 800).await;
        assert_eq!(
            s2,
            StatusCode::OK,
            "[{td_name}] settle 2 (overlapping): {b2}"
        );
        assert_eq!(
            b2["distributable_micros"],
            json!(2_700),
            "[{td_name}] the second window attributes ONLY the new burn — no re-batch"
        );
        let ledger = SqlLedger::new(td.pool.clone());
        let payable = normal_balance(
            &ledger,
            AccountCode::PayableDepositor,
            OwnerKind::Depositor,
            &dep,
        )
        .await;
        assert_eq!(
            payable,
            2 * 2_484,
            "[{td_name}] the books hold exactly two payouts"
        );
        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_sum(&rows, &dep, "earned_micros"),
            2 * 2_484,
            "[{td_name}] so do the caches"
        );
    }
}

/// A-053 leg 2: an upstream reporting usage 0/0 bills the reserve
/// estimate — capacity depletes, the key can serve free FOREVER no more.
#[tokio::test]
async fn a_zero_usage_report_burns_the_reserve_estimate() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock_usage(0, 0).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test"]).await;
        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            200_000,
            &mock.base_url,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;

        // Five zero-report requests burn 5 × 38,550 (the no-cap estimate)…
        for _ in 0..5 {
            let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] served: {resp}");
        }
        let rows = positions(&app, &token, &pool).await;
        let burned = pos_sum(&rows, &dep, "burned_micros");
        assert_eq!(
            burned,
            5 * 38_550,
            "[{td_name}] zero reports bill the estimate"
        );

        // …and the sixth REFUSES: remaining 7,250 < the 38,550 estimate.
        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(
            status,
            StatusCode::TOO_MANY_REQUESTS,
            "[{td_name}] floored burns exhaust the bucket: {resp}"
        );
    }
}

/// A-052 (founder-ruled in-doctrine): when the weighted pick cannot hold
/// the estimate, the request FALLS BACK to a sibling depositor's capacity
/// — members are served while any candidate can hold the estimate.
///
/// The setup is DETERMINISTIC (2026-11-26 — this pin used to flake on
/// CI): A is deposited ALONE and drained to exhaustion first, so every
/// hunt request is A's — exactly 10, no weighted pick involved — and B
/// is deposited only AFTER the hunt, holding its full 200,000. While B
/// rode the hunt too, the weighted pick drained it as well (5/6 of the
/// requests), and a long hunt left B under the estimate as well: the
/// fallback requests then had nothing left to serve with (≈19% per DB
/// leg, `P(NB(10, 1/6) ≥ 65)`).
#[tokio::test]
async fn a_refused_pick_falls_back_to_sibling_capacity() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock_a = spawn_mock(MockMode::NonStream).await;
        let mock_b = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test"]).await;
        let (dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            40_000,
            &mock_a.base_url,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;

        // max_tokens 100 ⇒ estimate 15,150; the honest mock meters
        // (11+7) × 150 = 2,700 per request.
        let body = json!({"model": "m-test", "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}]});
        const ESTIMATE: i64 = 15_150;

        // Hunt A's exhaustion: drive until A's remaining < ESTIMATE.
        // Every one of these requests must be OK (A holds it — it is the
        // pool's only depositor until the hunt ends).
        let mut exhausted = false;
        for _ in 0..400 {
            let (status, resp) = call(&app, chat_req(&raw, &body)).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] hunt phase: {resp}");
            let rows = positions(&app, &token, &pool).await;
            let a_remaining = pos_sum(&rows, &dep_a, "contributed_micros")
                - pos_sum(&rows, &dep_a, "burned_micros");
            if a_remaining < ESTIMATE {
                exhausted = true;
                break;
            }
        }
        assert!(
            exhausted,
            "[{td_name}] the small depositor must reach exhaustion within 400 requests"
        );
        assert!(
            mock_a.captured_count() >= 1,
            "[{td_name}] A served while it could"
        );

        // B enters ONLY NOW, with nothing spent — the sibling capacity
        // the pin needs is guaranteed by construction instead of by the
        // luck of the draws (setup fix, 2026-11-26).
        let (_dep_b, _kb) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            200_000,
            &mock_b.base_url,
        )
        .await;

        // THE PIN: A is exhausted, B holds its full 200,000 — five more
        // requests, ALL served, zero premature 429s. A pick on A is
        // refused and falls back to B; a pick on B is served directly.
        for _ in 0..5 {
            let (status, resp) = call(&app, chat_req(&raw, &body)).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "[{td_name}] fallback must serve while B holds the estimate: {resp}"
            );
        }
        assert!(
            mock_b.captured_count() >= 1,
            "[{td_name}] B took over A's refusals"
        );
    }
}

/// A-050 leg 2 honest state: a key declaring family "m" SERVES model
/// "m-test" (the hyphen-prefix contract) alongside the exact declarer —
/// no declaration by another depositor can exile it.
#[tokio::test]
async fn a_hyphen_prefix_declarer_serves_the_longer_model() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock_a = spawn_mock(MockMode::NonStream).await;
        let mock_b = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test", "m-mini"]).await;
        let (dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m",
            5_000_000,
            &mock_a.base_url,
        )
        .await;
        let (_dep_b, _kb) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            50_000,
            &mock_b.base_url,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;

        let body = json!({"model": "m-test", "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}]});
        for _ in 0..6 {
            let (status, resp) = call(&app, chat_req(&raw, &body)).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");
        }
        assert!(
            mock_a.captured_count() >= 1,
            "[{td_name}] the 'm' key serves m-test per the contract (weights 5M : 50k)"
        );
        assert!(
            mock_a.captured_count() + mock_b.captured_count() == 6,
            "[{td_name}] every request served exactly once"
        );
        // And its m-test burns book into ITS declared bucket.
        let rows = positions(&app, &token, &pool).await;
        let a_m_burn = rows
            .iter()
            .find(|r| r["depositor_id"] == json!(dep_a) && r["model_family"] == json!("m"))
            .map(|r| r["burned_micros"].as_i64().unwrap_or(0))
            .unwrap_or(0);
        assert_eq!(
            a_m_burn,
            mock_a.captured_count() as i64 * 2_700,
            "[{td_name}] A's burns book in A's 'm' bucket (the fixed 18-token mock: (11+7) × 150)"
        );
    }
}

/// A-047: an actual burn beyond the estimate lands at EXACTLY the
/// contribution (never past it), and the unbacked overage never becomes
/// distributable — settle pays the clamped slice only.
#[tokio::test]
async fn burn_clamps_at_the_contribution_and_overage_never_distributes() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock_usage(11, 4_000).await; // 601,650 µ$ metered
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let (org, pool) = org_and_root_pool(&app, &token, &["m-test"]).await;
        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            500_000,
            &mock.base_url,
        )
        .await;
        let raw = member_key(&app, &token, &org, &pool).await;
        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] served: {resp}");

        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_sum(&rows, &dep, "burned_micros"),
            500_000,
            "[{td_name}] the tracker clamps at contributed"
        );

        // Settle distributes the CLAMPED slice only — the 101,650 µ$
        // overage was served-but-unbacked; it must mint no payable.
        let now = db::now_ms();
        let (s, b) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s, StatusCode::OK, "[{td_name}] settle: {b}");
        assert_eq!(
            b["distributable_micros"],
            json!(500_000),
            "[{td_name}] distributable == the backed slice, not the metered 601,650"
        );
        assert_eq!(
            b["fee_micros"],
            json!(40_000),
            "[{td_name}] 8% of the backed slice"
        );
        assert_eq!(
            settle_total_for(&b, &dep),
            460_000,
            "[{td_name}] 92% of the backed slice"
        );
        let ledger = SqlLedger::new(td.pool.clone());
        let payable = normal_balance(
            &ledger,
            AccountCode::PayableDepositor,
            OwnerKind::Depositor,
            &dep,
        )
        .await;
        assert_eq!(
            payable, 460_000,
            "[{td_name}] the overage never became a payable"
        );
    }
}
