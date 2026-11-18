//! M9-C ADVERSARIAL AUDIT — Session C PoCs (doctrine-suite step 14).
//!
//! CONVENTION (M8 Session-C precedent): every test here ASSERTS THE
//! EXPLOITED STATE of a shipped finding. GREEN means the vulnerability is
//! PRESENT in the audited tree (v0.1.4, M9-B at cf3f991); the day a fix
//! session lands, the corresponding test flips RED — these are standing
//! pins, not regressions to silence. No existing test was edited.
//!
//! Every journey rides the PUBLIC API only (workflow step 12): pools,
//! deposits, member keys, chat and settle all go through the product's
//! own endpoints; raw SQL appears solely as ASSERTION-ONLY reads (the m5
//! sanction). Threat model per test: a member (chat surface), a
//! depositor (deposit families/provider/base_url), or an operator
//! calling settle — the personas named in the M9-C kickoff.
//!
//! PoC → finding → invariant map (full detail + trigger sequences live
//! in AUDIT.md rows A-047..A-054):
//! * A-047 (inv 6): estimate-vs-actual gap breaks the per-bucket ceiling
//!   - leg 1: array-content prompt zeroes the prompt estimate
//!   - leg 2: no-max_tokens body + long completion (default 256 est.)
//! * A-048 (inv 2/7): settle windows carry no settled-marking — the same
//!   rows settle twice, earnings double
//! * A-049 (inv 1/7, rule 7): /settle updates positions directly and
//!   never posts settle.distribute/settle.fee — books diverge
//! * A-050 (inv 8/5): bucket resolution is a free-string ordering game
//!   - leg 1: a second provider string steals the whole bucket
//!   - leg 2: a longer family silently exiles a prefix-declaring key
//! * A-051 (inv 5): a REVOKED key's family declaration keeps winning
//!   bucket resolution — the model 429s while funded keys idle
//! * A-052 (inv 5/8): a picked-then-refused key 429s with no fallback to
//!   spendable depositors in the same bucket
//! * A-053 (inv 1/6): upstream-reported token counts are trusted
//!   - leg 1: two hostile-usage rows overflow i64 in /settle (panic)
//!   - leg 2: a zero-usage upstream serves free forever
//! * A-054: attempted-clean probes (fee band edges, window validation,
//!   mid-window deposits, rows↔positions reconciliation) — these assert
//!   HONEST behavior and should stay green forever.

mod common;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use common::{Captured, MockMode, MockUpstream, TEST_UPSTREAM_KEY, spawn_mock, test_app, test_dbs};
use futures_util::FutureExt;
use serde_json::{Value, json};
use sqlx::Row;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tributary_gateway::db;
use tributary_ledger::chart::{AccountCode, OwnerKind, PLATFORM_OWNER_ID};
use tributary_ledger::{Ledger, SqlLedger};

// ---------------------------------------------------------------------------
// Harness (m9_doctrine.rs shapes, kept local so this file owns its rig)
// ---------------------------------------------------------------------------

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
async fn call(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
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
async fn bootstrap_admin_and_login(pool: &db::Db, app: &Router) -> String {
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

/// A fresh org through the public API.
async fn new_org(app: &Router, token: &str) -> String {
    let (status, org) = call(
        app,
        api(
            "POST",
            "/api/v1/orgs",
            token,
            Some(&json!({"name": "M9C Audit Org", "slug": format!("m9c-{}", db::new_id())})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "org: {org}");
    org["id"].as_str().unwrap().to_string()
}

/// A ROOT (depositor-mode) pool through the public API.
async fn root_pool(app: &Router, token: &str, org_id: &str, models: &[&str]) -> String {
    let (status, pool) = call(
        app,
        api(
            "POST",
            "/api/v1/pools",
            token,
            Some(&json!({
                "org_id": org_id,
                "name": "Audit Root Pool",
                "kind": "root",
                "model_allowlist": models,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "root pool: {pool}");
    pool["id"].as_str().unwrap().to_string()
}

/// A deposit through the M9 contract path; provider is a PARAMETER here
/// because A-050 attacks exactly that free string. Returns
/// (depositor_id, provider_key_id).
#[allow(clippy::too_many_arguments)] // test rig: explicit args beat a builder struct
async fn deposit(
    app: &Router,
    token: &str,
    pool_id: &str,
    provider: &str,
    family: &str,
    contributed_micros: i64,
    base_url: &str,
    existing_depositor: Option<&str>,
) -> (String, String) {
    let mut body = json!({
        "provider": provider,
        "key": TEST_UPSTREAM_KEY,
        "base_url": base_url,
        "model_families": [family],
        "contributed_micros": contributed_micros,
    });
    if let Some(id) = existing_depositor {
        body["depositor_id"] = json!(id);
    }
    let (status, resp) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/deposits"),
            token,
            Some(&body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "deposit (M9 contract): {resp}");
    (
        resp["depositor_id"].as_str().unwrap().to_string(),
        resp["provider_key_id"].as_str().unwrap().to_string(),
    )
}

/// A member + pool key on the pool (existing V1 public path).
async fn member_key(app: &Router, token: &str, org_id: &str, pool_id: &str) -> (String, String) {
    let email = format!("m9c-{}@trib.test", db::new_id());
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
    let member_id = member["id"].as_str().unwrap().to_string();
    let (status, key) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/keys"),
            token,
            Some(&json!({"user_id": member_id})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "member key: {key}");
    (
        key["raw"].as_str().unwrap().to_string(),
        key["id"].as_str().unwrap().to_string(),
    )
}

/// POST /settle with an explicit window and fee.
async fn settle(
    app: &Router,
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

/// GET /bucket-positions (the off-chain books read).
async fn positions(app: &Router, token: &str, pool_id: &str) -> Vec<Value> {
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

/// One numeric field of one depositor's position row (0 when absent).
fn pos_field(rows: &[Value], depositor: &str, field: &str) -> i64 {
    rows.iter()
        .find(|r| r["depositor_id"] == json!(depositor))
        .map(|r| r[field].as_i64().unwrap_or(0))
        .unwrap_or(0)
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

/// Assertion-only read: attributed rows grouped by (depositor, provider,
/// family) with their cost sums — reconciliation truth for the books.
async fn attributed_costs(pool: &db::Db, pool_id: &str) -> Vec<(String, String, String, i64)> {
    let sql = pool.sql(
        "SELECT depositor_id, bucket_provider, bucket_model_family, \
          CAST(COALESCE(SUM(cost_micros), 0) AS BIGINT) AS c \
          FROM usage_events WHERE pool_id = ? AND depositor_id IS NOT NULL \
          GROUP BY depositor_id, bucket_provider, bucket_model_family",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(pool.pool())
        .await
        .expect("attributed-cost read");
    rows.iter()
        .map(|row| {
            (
                row.get::<String, _>("depositor_id"),
                row.get::<String, _>("bucket_provider"),
                row.get::<String, _>("bucket_model_family"),
                row.get::<i64, _>("c"),
            )
        })
        .collect()
}

/// A mock upstream that reports ATTACKER-CHOSEN token counts — the shape
/// A-053 needs, because in M9 the gateway prices whatever usage the
/// upstream (whose base_url a deposit names) reports. Same capture
/// contract as `common::spawn_mock`.
// #[allow(clippy::type_complexity)] — the handler's state tuple mirrors
// common::spawn_mock's (mode, captured) shape with the two usage numbers
// added; a struct here would diverge from the harness being mirrored.
#[allow(clippy::type_complexity)]
async fn spawn_mock_usage(prompt_tokens: i64, completion_tokens: i64) -> MockUpstream {
    let captured: Arc<Mutex<Vec<Captured>>> = Arc::new(Mutex::new(Vec::new()));
    let state = (prompt_tokens, completion_tokens, captured.clone());
    let app = Router::new()
        .route(
            "/chat/completions",
            post(
                |State(state): State<(i64, i64, Arc<Mutex<Vec<Captured>>>)>,
                 headers: HeaderMap,
                 body: Bytes| async move {
                    {
                        let mut guard = state.2.lock().expect("mock capture lock");
                        guard.push(Captured {
                            auth: headers
                                .get(header::AUTHORIZATION)
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                            body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                        });
                    }
                    Json(json!({
                        "id": "chatcmpl-m9c",
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

/// The plain chat body the doctrine suite uses (string content, no
/// declared cap) — estimate 1+256 tokens = 38,550 µ$ at the v0 slice.
fn plain_chat_body(model: &str) -> Value {
    json!({"model": model, "messages": [{"role": "user", "content": "hi"}]})
}

// ---------------------------------------------------------------------------
// A-047 — per-bucket overdraw via the estimate-vs-actual gap (invariant 6)
// ---------------------------------------------------------------------------

/// A-047 leg 1: a perfectly legal OpenAI body whose `content` is an
/// ARRAY of parts zeroes the prompt estimate (`estimate_tokens` reads
/// string content only), and `max_tokens: 1` caps the completion
/// estimate — total reserve 2 tokens = 300 µ$, exactly the contribution.
/// The honest upstream then meters 18 tokens = 2,700 µ$ and the settle
/// half of the guard applies `actual - estimate` with NO ceiling:
/// burned lands 9× past contributed in ONE request.
// #[tokio::test] — in-process async test runtime (standard repo harness)
#[tokio::test]
async fn poc_a047_leg1_overdraw_via_non_string_prompt_content() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            300,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        let body = json!({
            "model": "m-test",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "please answer at length, thoroughly, with examples and citations"}
            ]}]
        });
        let (status, resp) = call(&app, chat_req(&raw, &body)).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "[{td_name}] the adversarial request must be SERVED (the reserve passed): {resp}"
        );
        common::wait_for_usage(&td.pool, &key_id, 1).await;

        let rows = positions(&app, &token, &pool).await;
        let contributed = pos_field(&rows, &dep, "contributed_micros");
        let burned = pos_field(&rows, &dep, "burned_micros");
        assert_eq!(contributed, 300, "[{td_name}] fixture: contributed 300 µ$");
        assert_eq!(
            burned, 2_700,
            "[{td_name}] the mock metered (11+7) tokens × 150 µ$"
        );
        assert!(
            burned > contributed,
            "[{td_name}] INVARIANT 6 BROKEN: burned {burned} µ$ > contributed {contributed} µ$ in one request",
        );

        // Aftermath: the key is exiled by eligibility — the ceiling was
        // only ever enforced at reserve time, never at settle.
        let (status2, body2) = call(&app, chat_req(&raw, &body)).await;
        assert_eq!(
            status2,
            StatusCode::TOO_MANY_REQUESTS,
            "[{td_name}] the overdrawn key must now refuse: {body2}"
        );
    }
}

/// A-047 leg 2: no adversarial body shape at all — a plain string prompt
/// with NO declared cap reserves the default 256-token completion, but
/// an upstream that simply generates long (4,000 tokens) settles 601,650
/// µ$ against a 500,000 µ$ contribution. The guard's own comment calls
/// the estimate "deliberately conservative"; it is not an upper bound.
#[tokio::test]
async fn poc_a047_leg2_overdraw_via_default_completion_estimate() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock_usage(11, 4_000).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            500_000,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] served: {resp}");
        common::wait_for_usage(&td.pool, &key_id, 1).await;

        let rows = positions(&app, &token, &pool).await;
        let contributed = pos_field(&rows, &dep, "contributed_micros");
        let burned = pos_field(&rows, &dep, "burned_micros");
        assert_eq!(contributed, 500_000, "[{td_name}] fixture");
        assert_eq!(
            burned,
            (11 + 4_000) * 150,
            "[{td_name}] actual (11+4000) × 150"
        );
        assert!(
            burned > contributed,
            "[{td_name}] INVARIANT 6 BROKEN: burned {burned} > contributed {contributed} — one long completion overdraws the bucket",
        );
    }
}

// ---------------------------------------------------------------------------
// A-048 — settle windows carry no settled-marking (invariants 2 and 7)
// ---------------------------------------------------------------------------

/// A-048: `attribute_period_burns` re-reads whatever rows fall in the
/// window; nothing marks a row settled and nothing remembers a window.
/// Calling /settle twice over the same period re-batches the same burns
/// and credits `earned_micros` TWICE — the depositor earns a second
/// time in a period where their credits burned nothing (retry after a
/// client timeout is enough; no malicious payload required).
#[tokio::test]
async fn poc_a048_double_settle_double_credits_earnings() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;
        for _ in 0..3 {
            let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");
        }
        common::wait_for_usage(&td.pool, &key_id, 3).await;
        // Burned total: 3 × (11+7) × 150 = 8,100 µ$.

        let now = db::now_ms();
        let (s1, b1) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s1, StatusCode::OK, "[{td_name}] settle #1: {b1}");
        assert_eq!(
            b1["distributable_micros"],
            json!(8_100),
            "[{td_name}] burn total"
        );
        assert_eq!(b1["fee_micros"], json!(648), "[{td_name}] 8% fee");
        let first_payout = settle_total_for(&b1, &dep);
        assert_eq!(first_payout, 7_452, "[{td_name}] after-fee payout");

        // THE EXPLOIT: the identical window settles again — same rows,
        // same split, credited a second time.
        let (s2, b2) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s2, StatusCode::OK, "[{td_name}] settle #2 accepted: {b2}");
        assert_eq!(
            b2["distributable_micros"],
            json!(8_100),
            "[{td_name}] the same rows re-batched — no window marking"
        );
        assert_eq!(
            settle_total_for(&b2, &dep),
            7_452,
            "[{td_name}] the same payout computed a second time"
        );

        let rows = positions(&app, &token, &pool).await;
        let earned = pos_field(&rows, &dep, "earned_micros");
        assert_eq!(
            earned, 14_904,
            "[{td_name}] INVARIANTS 2/7 BROKEN: earned {earned} = 2 × 7,452 — payables no longer equal the distributable (7,452)",
        );
    }
}

// ---------------------------------------------------------------------------
// A-049 — /settle never posts the books (invariant 1, rule 7)
// ---------------------------------------------------------------------------

/// A-049: the deposits path posts `deposit.capacity` (claims.contributed
/// grows on the M7 books), but /settle updates
/// `depositor_bucket_positions.earned_micros` directly and posts
/// NEITHER settle.distribute NOR settle.fee. After a real deposit,
/// traffic and settle, the depositor's payable.depositor account does
/// not even EXIST while their position shows earnings — the books are
/// structurally blind to settled payables (and to the operator fee),
/// so rule 7's nightly solvency check cannot see them either.
#[tokio::test]
async fn poc_a049_settle_books_diverge_from_positions() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;
        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");
        common::wait_for_usage(&td.pool, &key_id, 1).await;

        let now = db::now_ms();
        let (s, b) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s, StatusCode::OK, "[{td_name}] settle: {b}");
        assert_eq!(
            settle_total_for(&b, &dep),
            2_484,
            "[{td_name}] payout 0.92 × 2,700"
        );
        assert_eq!(b["fee_micros"], json!(216), "[{td_name}] fee 0.08 × 2,700");

        // The books, read through the PUBLIC ledger trait only.
        let ledger = SqlLedger::new(td.pool.clone());
        let claims_id = ledger
            .account_id(AccountCode::ClaimsContributed, OwnerKind::Depositor, &dep)
            .await
            .expect("claims account lookup")
            .expect("the DEPOSIT did post deposit.capacity");
        let claims = ledger
            .account_balance(&claims_id)
            .await
            .expect("claims balance read")
            .normal
            .micros();
        assert_eq!(claims, 5_000_000, "[{td_name}] books took the deposit");

        let payable = ledger
            .account_id(AccountCode::PayableDepositor, OwnerKind::Depositor, &dep)
            .await
            .expect("payable lookup");
        assert!(
            payable.is_none(),
            "[{td_name}] INVARIANT 1/7 BROKEN: settle never posted — payable.depositor does not even exist while the position shows earned",
        );
        let fee_account = ledger
            .account_id(
                AccountCode::EquityOperator,
                OwnerKind::Operator,
                PLATFORM_OWNER_ID,
            )
            .await
            .expect("operator account lookup");
        assert!(
            fee_account.is_none(),
            "[{td_name}] the 216 µ$ fee exists only in the settle response — equity.operator never saw it",
        );

        let rows = positions(&app, &token, &pool).await;
        let earned = pos_field(&rows, &dep, "earned_micros");
        assert_eq!(earned, 2_484, "[{td_name}] the position DID get credited");
        // The divergence, stated: earned 2,484 on the positions, payable
        // absent (0) on the books — two truths for the same money.
    }
}

// ---------------------------------------------------------------------------
// A-050 — bucket resolution is a free-string ordering game (invariants
// 8 and 5)
// ---------------------------------------------------------------------------

/// A-050 leg 1: bucket resolution ties break by LARGER provider string,
/// and `provider` is an unchecked deposit payload string. A depositor
/// contributing 1% under provider "zzz" captures 100% of the model's
/// traffic from a depositor holding 99% of the weight under "openai" —
/// every request dials the zzz deposit's base_url. Invariant 8's
/// "realized ≝ declared weights" is inverted outright.
#[tokio::test]
async fn poc_a050_leg1_provider_string_captures_the_bucket() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock_a = spawn_mock(MockMode::NonStream).await;
        let mock_b = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock_a.base_url,
            None,
        )
        .await;
        let (dep_b, _kb) = deposit(
            &app,
            &token,
            &pool,
            "zzz",
            "m-test",
            50_000,
            &mock_b.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        // max_tokens 100 keeps the reserve (15,150 µ$) inside B's tiny
        // 50,000 µ$ contribution for all six requests.
        let body = json!({"model": "m-test", "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}]});
        for _ in 0..6 {
            let (status, resp) = call(&app, chat_req(&raw, &body)).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");
        }
        common::wait_for_usage(&td.pool, &key_id, 6).await;

        assert_eq!(
            mock_b.captured_count(),
            6,
            "[{td_name}] the zzz key took ALL traffic"
        );
        assert_eq!(
            mock_a.captured_count(),
            0,
            "[{td_name}] the 99%-weight key took NONE"
        );

        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_field(&rows, &dep_a, "burned_micros"),
            0,
            "[{td_name}] A never served"
        );
        assert_eq!(
            pos_field(&rows, &dep_b, "burned_micros"),
            16_200,
            "[{td_name}] B served all six"
        );

        let sql = td.pool.sql(
            "SELECT bucket_provider, COUNT(*) AS n FROM usage_events WHERE pool_id = ? GROUP BY bucket_provider",
        );
        let prov_rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&pool)
            .fetch_all(td.pool.pool())
            .await
            .expect("bucket-provider read");
        assert_eq!(
            prov_rows.len(),
            1,
            "[{td_name}] one bucket served everything"
        );
        assert_eq!(
            prov_rows[0].get::<String, _>("bucket_provider"),
            "zzz",
            "[{td_name}] every burn attributed into the attacker's bucket"
        );
        assert_eq!(prov_rows[0].get::<i64, _>("n"), 6);
    }
}

/// A-050 leg 2: same provider, two families. The suite header's contract
/// says a key serves model M iff some declared family equals M or is a
/// hyphen-prefix of M — family "m" serves "m-test" by that sentence.
/// But resolution picks the LONGEST declaring family and then admits
/// only keys declaring EXACTLY it: the "m" key is exiled from "m-test"
/// (and, symmetrically, a longer declaration monopolizes any model).
#[tokio::test]
async fn poc_a050_leg2_longest_family_exiles_the_prefix_declarer() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock_a = spawn_mock(MockMode::NonStream).await;
        let mock_b = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test", "m-mini"]).await;

        let (dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m",
            5_000_000,
            &mock_a.base_url,
            None,
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
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        let body = json!({"model": "m-test", "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}]});
        for _ in 0..6 {
            let (status, resp) = call(&app, chat_req(&raw, &body)).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");
        }
        common::wait_for_usage(&td.pool, &key_id, 6).await;
        assert_eq!(
            mock_a.captured_count(),
            0,
            "[{td_name}] the 'm' key declares a family that SERVES m-test per the contract — yet served nothing"
        );
        assert_eq!(
            mock_b.captured_count(),
            6,
            "[{td_name}] the longer family monopolizes"
        );
        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_field(&rows, &dep_a, "burned_micros"),
            0,
            "[{td_name}] A's weight never counted"
        );

        // The exiled key still serves models where ITS family is the
        // longest match — the exile is per-model, i.e. the "serves"
        // contract sentence depends on what OTHER deposits declare.
        let mini = json!({"model": "m-mini", "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}]});
        for _ in 0..2 {
            let (status, resp) = call(&app, chat_req(&raw, &mini)).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] m-mini chat: {resp}");
        }
        assert_eq!(
            mock_a.captured_count(),
            2,
            "[{td_name}] m-mini resolves to the 'm' bucket"
        );
    }
}

// ---------------------------------------------------------------------------
// A-051 — revoked keys keep winning bucket resolution (invariant 5)
// ---------------------------------------------------------------------------

/// A-051: bucket resolution scans depositor keys with NO status filter;
/// only per-key ELIGIBILITY is revocation-aware. Chain: deposit a key
/// under provider "zzz" declaring the family (winning resolution by
/// A-050's ordering), then REVOKE it through the public DELETE path —
/// its zombie declaration still owns the bucket, eligibility finds zero
/// keys, and every request for the model 429s while a healthy, funded
/// key for the same model sits idle in the pool. Stranding, not theft.
#[tokio::test]
async fn poc_a051_revoked_key_zombie_family_bricks_the_model() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock_a = spawn_mock(MockMode::NonStream).await;
        let mock_b = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock_a.base_url,
            None,
        )
        .await;
        let (_dep_b, key_b) = deposit(
            &app,
            &token,
            &pool,
            "zzz",
            "m-test",
            50_000,
            &mock_b.base_url,
            None,
        )
        .await;
        let (raw, _key_id) = member_key(&app, &token, &org, &pool).await;

        // Revoke the zzz key through the public path (single-admin org:
        // the 4-eyes gate auto-passes with the audit marker, D33/A-026).
        let (status, resp) = call(
            &app,
            api(
                "DELETE",
                &format!("/api/v1/pools/{pool}/provider-keys/{key_b}"),
                &token,
                None,
            ),
        )
        .await;
        assert!(
            status.is_success(),
            "[{td_name}] revoke via public path: {status} {resp}"
        );

        let body = json!({"model": "m-test", "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}]});
        for _ in 0..5 {
            let (status, resp) = call(&app, chat_req(&raw, &body)).await;
            assert_eq!(
                status,
                StatusCode::TOO_MANY_REQUESTS,
                "[{td_name}] every request must refuse: {resp}"
            );
            let text = resp.to_string().to_lowercase();
            assert!(
                text.contains("zzz/m-test"),
                "[{td_name}] the refusal must name the zombie bucket: {resp}"
            );
        }
        assert_eq!(
            mock_a.captured_count(),
            0,
            "[{td_name}] INVARIANT 5 BROKEN (spirit): a healthy key with 5,000,000 µ$ of capacity never dialed"
        );
        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_field(&rows, &dep_a, "burned_micros"),
            0,
            "[{td_name}] A stranded"
        );
        assert_eq!(
            pos_field(&rows, &dep_a, "contributed_micros"),
            5_000_000,
            "[{td_name}] A's capacity exists — the model is bricked anyway"
        );
    }
}

// ---------------------------------------------------------------------------
// A-052 — picked-then-refused key 429s with no fallback (invariants 5/8)
// ---------------------------------------------------------------------------

/// A-052: selection makes ONE weighted pick, then reserves against it;
/// on refusal the whole request 429s — it never retries the other
/// depositors of the same bucket. Fixture: A holds 40,000 µ$ (2/3
/// weight) and burns down to where the 15,150 µ$ estimate no longer
/// fits; B holds 200,000 µ$ untouched. Members are refused while B's
/// key could serve the very request that was refused.
#[tokio::test]
async fn poc_a052_reserve_refusal_has_no_fallback() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock_a = spawn_mock(MockMode::NonStream).await;
        let mock_b = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            40_000,
            &mock_a.base_url,
            None,
        )
        .await;
        let (dep_b, _kb) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            200_000,
            &mock_b.base_url,
            None,
        )
        .await;
        let (raw, _key_id) = member_key(&app, &token, &org, &pool).await;

        let body = json!({"model": "m-test", "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}]});
        const ESTIMATE: i64 = 15_150; // (1 prompt est + 100 cap) × 150 µ$

        let mut refusal_with_capacity = 0;
        let mut ok_count = 0;
        for _ in 0..400 {
            let (status, resp) = call(&app, chat_req(&raw, &body)).await;
            if status == StatusCode::OK {
                ok_count += 1;
                continue;
            }
            assert_eq!(
                status,
                StatusCode::TOO_MANY_REQUESTS,
                "[{td_name}] only 429s expected: {resp}"
            );
            let rows = positions(&app, &token, &pool).await;
            let a_remaining = pos_field(&rows, &dep_a, "contributed_micros")
                - pos_field(&rows, &dep_a, "burned_micros");
            let b_remaining = pos_field(&rows, &dep_b, "contributed_micros")
                - pos_field(&rows, &dep_b, "burned_micros");
            if a_remaining < ESTIMATE && b_remaining >= ESTIMATE {
                // THE EXPLOIT STATE: A was picked and refused; B could
                // have served this exact request but was never asked.
                refusal_with_capacity += 1;
            }
        }
        assert!(
            refusal_with_capacity >= 1,
            "[{td_name}] INVARIANT 5/8 BROKEN: no 429 ever landed while B still had ≥ estimate spare ({refusal_with_capacity} seen, {ok_count} served)",
        );
        assert!(mock_a.captured_count() >= 1, "[{td_name}] A served first");
        assert!(mock_b.captured_count() >= 1, "[{td_name}] B served too");
    }
}

// ---------------------------------------------------------------------------
// A-053 — upstream-reported token counts are trusted (invariants 1/6)
// ---------------------------------------------------------------------------

/// i64::MAX / 150 rounded down — the largest per-request cost that does
/// not saturate `price_tokens`; two such rows overflow the i64 sum the
/// settle endpoint accumulates per bucket.
const HUGE_COMPLETION_TOKENS: i64 = 61_489_146_912_365_017;

/// A-053 leg 1: the gateway prices whatever `usage` the upstream
/// reports, and the settle endpoint sums per-bucket costs in i64. Two
/// hostile-usage rows (one per depositor — each key serves once before
/// its own overdraw exiles it) overflow that sum: in a debug build the
/// arithmetic panics inside POST /settle; in a release build it wraps
/// negative and `clamp(0, dist)` panics with min > max. Either way the
/// window — and every future window containing the rows — can never
/// settle: a permanent settlement bricking from two requests.
#[tokio::test]
async fn poc_a053_leg1_hostile_usage_overflow_bricks_settle() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock_usage(1, HUGE_COMPLETION_TOKENS).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        // Two depositors in one bucket: the first request overdrafts
        // whoever serves it (A-047), exiling that key — the second
        // request must then land on the other depositor. Two rows.
        let (_dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;
        let (_dep_b, _kb) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, _key_id) = member_key(&app, &token, &org, &pool).await;
        for _ in 0..2 {
            let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] served: {resp}");
        }

        let now = db::now_ms();
        let req = api(
            "POST",
            &format!("/api/v1/pools/{pool}/settle"),
            &token,
            Some(&json!({"from_ms": 0, "to_ms": now + 60_000, "fee_bps": 800})),
        );
        // The overflow panics inside the handler; catch it so the PoC
        // can ASSERT the brick instead of dying with it.
        let outcome = AssertUnwindSafe(async { call(&app, req).await })
            .catch_unwind()
            .await;
        match outcome {
            Err(_panic_payload) => {
                // Exploit confirmed: settle PANICS on the overflow.
            }
            Ok((status, body)) => {
                panic!(
                    "[{td_name}] expected settle to panic or fail on i64 overflow, got {status}: {body}"
                );
            }
        }
    }
}

/// A-053 leg 2: a deposit whose upstream reports usage 0/0 meters every
/// request at cost 0 — the key serves free FOREVER (burned never moves,
/// eligibility never fails), and because member token quotas aggregate
/// the same reported tokens, a colluding upstream is also a member-quota
/// bypass primitive. Zero burn correctly earns zero (invariant 2 holds)
/// — the abuse is unbounded free capacity, not stolen earnings.
#[tokio::test]
async fn poc_a053_leg2_zero_usage_upstream_serves_free_forever() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock_usage(0, 0).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;
        for _ in 0..20 {
            let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] request served: {resp}");
        }
        common::wait_for_usage(&td.pool, &key_id, 20).await;

        let rows = positions(&app, &token, &pool).await;
        assert_eq!(
            pos_field(&rows, &dep, "burned_micros"),
            0,
            "[{td_name}] twenty served requests, zero burn — capacity never exhausts"
        );
        // The 21st still serves: nothing about this key can ever refuse.
        let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] free forever: {resp}");
        assert_eq!(
            mock.captured_count(),
            21,
            "[{td_name}] all requests reached upstream"
        );
    }
}

// ---------------------------------------------------------------------------
// A-054 — attempted-clean probes (these assert HONEST behavior)
// ---------------------------------------------------------------------------

/// A-054a: the fee band (rule 8) holds at its edges — 499 and 1001 are
/// refused, 500 and 1000 are accepted — and the window validation
/// refuses empty/reversed windows. Clean.
#[tokio::test]
async fn clean_fee_band_edges_and_window_validation_hold() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let now = db::now_ms();
        for fee in [499i64, 1001] {
            let (status, body) = settle(&app, &token, &pool, 0, now + 60_000, fee).await;
            assert!(
                status.is_client_error(),
                "[{td_name}] fee {fee} outside the 5–10% band must 4xx: {status} {body}"
            );
        }
        for fee in [500i64, 1000] {
            let (status, body) = settle(&app, &token, &pool, 0, now + 60_000, fee).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "[{td_name}] fee {fee} in-band: {body}"
            );
            assert_eq!(
                body["distributable_micros"],
                json!(0),
                "[{td_name}] empty window"
            );
        }
        let (status, body) = settle(&app, &token, &pool, now, now, 800).await;
        assert!(
            status.is_client_error(),
            "[{td_name}] from_ms == to_ms must 4xx: {status} {body}"
        );
    }
}

/// A-054b: composition passes that came back CLEAN — a deposit landing
/// mid-window (weights mutated mid-window) earns nothing for the new
/// depositor (invariant 2 holds under composition), and the position
/// burned_micros still reconcile exactly to the attributed usage rows
/// per (depositor, bucket) — the batch attributes every row and the
/// tracker matches the rows to the micro-unit.
#[tokio::test]
async fn clean_mid_window_deposit_and_row_reconciliation_hold() {
    for td in test_dbs().await {
        let td_name = td.name;
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep_a, _ka) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;
        for _ in 0..2 {
            let (status, resp) = call(&app, chat_req(&raw, &plain_chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {resp}");
        }
        common::wait_for_usage(&td.pool, &key_id, 2).await;
        // Mid-window deposit: a second depositor lands AFTER the burns.
        let (dep_b, _kb) = deposit(
            &app,
            &token,
            &pool,
            "openai",
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;

        let now = db::now_ms();
        let (s, b) = settle(&app, &token, &pool, 0, now + 60_000, 800).await;
        assert_eq!(s, StatusCode::OK, "[{td_name}] settle: {b}");
        assert_eq!(
            settle_total_for(&b, &dep_b),
            0,
            "[{td_name}] B burned nothing — earns nothing (mid-window deposit)"
        );
        assert_eq!(
            settle_total_for(&b, &dep_a),
            4_968,
            "[{td_name}] A's burn 5,400 − 8% fee"
        );

        let rows = positions(&app, &token, &pool).await;
        for (depositor, _provider, _family, cost) in attributed_costs(&td.pool, &pool).await {
            let burned = pos_field(&rows, &depositor, "burned_micros");
            assert_eq!(
                burned, cost,
                "[{td_name}] {depositor}: position burned must equal Σ attributed row costs"
            );
        }
        assert_eq!(
            pos_field(&rows, &dep_a, "burned_micros"),
            5_400,
            "[{td_name}] A rows"
        );
        assert_eq!(
            pos_field(&rows, &dep_b, "burned_micros"),
            0,
            "[{td_name}] B rows"
        );
    }
}
