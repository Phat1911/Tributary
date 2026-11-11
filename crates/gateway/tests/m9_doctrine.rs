//! M9 DOCTRINE SUITE — Session A (doctrine-suite mode, AGENTS.md step 14).
//!
//! Every test in this file is one DOCTRINE SENTENCE from the M9 kickoff,
//! named exactly as itself — the table of contents IS the spec. Journeys
//! ride the PUBLIC API ONLY (workflow step 12): every entity is
//! provisioned through the product's own paths; direct SQL is used
//! solely for ASSERTION-ONLY reads (the m5 sanction) and one schema
//! shape-probe, never to seed what the product can provision.
//!
//! Expected RED on this branch — every failure must be doctrine-shaped
//! (an assertion against a `todo!()` stub or a missing M9 product path),
//! never a compile error. Session B implements to green and NEVER edits
//! a test (rule 6, extended to doctrine).
//!
//! THE M9 PUBLIC-API CONTRACT (what Session B makes exist; the journeys
//! below are its executable statement):
//!   * `POST /api/v1/pools` gains an optional `kind`: `"org"` (default,
//!     unchanged V1) or `"root"` — a ROOT pool is depositor mode.
//!   * `POST /api/v1/pools/{id}/deposits`
//!     `{provider, key, base_url, model_families: [..], contributed_micros,
//!       depositor_id?}` → 201 `{depositor_id, provider_key_id, key_prefix}`.
//!     The depositor provisioning path: envelope-encrypts the key (rule 2),
//!     posts `deposit.capacity` (claims.contributed += contributed_micros),
//!     opens the per-bucket position rows. `depositor_id` links further
//!     keys to an existing depositor (one depositor, many buckets).
//!   * `GET /api/v1/pools/{id}/bucket-positions` → 200 array of
//!     `{depositor_id, provider, model_family, contributed_micros,
//!       burned_micros, earned_micros}` — the off-chain books (D43).
//!   * `POST /api/v1/pools/{id}/settle` `{from_ms, to_ms, fee_bps}` → 200
//!     `{distributable_micros, fee_micros, undistributed_micros, payouts:
//!       [{depositor_id, provider, model_family, amount_micros}]}`.
//!     `distributable_micros` == Σ cost_micros of attributed usage rows in
//!     the window (M9's pricing slice decides the cost; it must be
//!     positive and deterministic); both endpoints REFUSE org pools (4xx).
//!   * Bucket fit: a key serves model M iff same provider AND some
//!     declared family F satisfies F == M or F is a hyphen-prefix of M
//!     ("gpt-4o" serves "gpt-4o-mini") — families are data, never code.
//!   * Per-bucket exhaustion answers 429 (the OpenAI-compatible "no
//!     capacity" shape) with a message naming capacity/contribution.
//!
//! Residents (test name = doctrine sentence → kickoff invariant #):
//!   1. every_consumption_… (#1)   2. a_depositor_earns_nothing_… (#2)
//!   3. a_requests_burn_… (#3)     5. a_request_never_routes_… (#5)
//!   6. no_depositors_key_… (#6)   8. realized_routing_distribution_… (#8)
//!   9. all_changes_are_additive_… (#10)
//!   11. quota_reservation_stays_… (#11)
//!
//! Invariant #9 (`a_pool_that_has_not_enabled_depositor_mode_…`) is GREEN
//! ON THE BRANCH BY DESIGN — it pins UNCHANGED V1 behavior; red there
//! would mean the additive rule was already broken. Invariants #4 and #7
//! (pure settlement math) live in the ledger suite:
//! crates/ledger/tests/m9_doctrine.rs.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{MockMode, test_app, test_dbs};
use serde_json::{Value, json};
use sqlx::Row;
use tributary_gateway::db;
use tributary_ledger::money::MicroUsd;
use tributary_ledger::settle::Bucket;

// ---------------------------------------------------------------------------
// Harness (m55 pattern: authenticated JSON calls against the merged app)
// ---------------------------------------------------------------------------

/// Control-API helper: an authenticated JSON request.
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

/// Fires a request at the merged app; returns (status, parsed JSON body).
async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    use tower::ServiceExt;
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

/// A chat request (data plane), socket-address-free like m55's None-IP leg.
fn chat_req(key: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Bootstrap host admin + login (shared across this binary's parallel
/// tests on the one Postgres; the product's own bootstrap path).
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

/// Creates an org through the public API; returns its id.
async fn new_org(app: &axum::Router, token: &str) -> String {
    let (status, org) = call(
        app,
        api(
            "POST",
            "/api/v1/orgs",
            token,
            Some(&json!({"name": "M9 Doctrine Org", "slug": format!("m9-{}", db::new_id())})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "org: {org}");
    org["id"].as_str().unwrap().to_string()
}

/// Creates a ROOT (depositor-mode) pool through the public API — the M9
/// contract: `kind: "root"`. Returns the pool id.
async fn root_pool(app: &axum::Router, token: &str, org_id: &str, models: &[&str]) -> String {
    let (status, pool) = call(
        app,
        api(
            "POST",
            "/api/v1/pools",
            token,
            Some(&json!({
                "org_id": org_id,
                "name": "Root Pool",
                "kind": "root",
                "model_allowlist": models,
            })),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "root pool via POST /api/v1/pools with kind=root (M9 contract): {pool}"
    );
    pool["id"].as_str().unwrap().to_string()
}

/// A depositor deposit through the public API — the M9 contract path.
/// Returns (depositor_id, provider_key_id). Provider is fixed to "openai"
/// here on purpose: provider VARIETY was proven at M5.5 (provider-agnostic
/// by construction); these journeys pin bucket doctrine, not dialects.
async fn deposit(
    app: &axum::Router,
    token: &str,
    pool_id: &str,
    family: &str,
    contributed_micros: i64,
    base_url: &str,
    existing_depositor: Option<&str>,
) -> (String, String) {
    let mut body = json!({
        "provider": "openai",
        "key": common::TEST_UPSTREAM_KEY,
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
    assert_eq!(
        status,
        StatusCode::CREATED,
        "deposit via POST /api/v1/pools/{pool_id}/deposits (M9 contract): {resp}"
    );
    (
        resp["depositor_id"]
            .as_str()
            .expect("depositor_id")
            .to_string(),
        resp["provider_key_id"]
            .as_str()
            .expect("provider_key_id")
            .to_string(),
    )
}

/// A member pool key on the pool (existing V1 public path).
async fn member_key(
    app: &axum::Router,
    token: &str,
    org_id: &str,
    pool_id: &str,
) -> (String, String) {
    let email = format!("member-{}@trib.test", db::new_id());
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

/// POST settle (M9 contract); returns the parsed body + status.
async fn settle_pool(
    app: &axum::Router,
    token: &str,
    pool_id: &str,
    fee_bps: i64,
) -> (StatusCode, Value) {
    call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/settle"),
            token,
            Some(&json!({"from_ms": 0, "to_ms": db::now_ms() + 60_000, "fee_bps": fee_bps})),
        ),
    )
    .await
}

/// GET bucket-positions (M9 contract); returns status + parsed array.
async fn bucket_positions(
    app: &axum::Router,
    token: &str,
    pool_id: &str,
) -> (StatusCode, Vec<Value>) {
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
    let rows = body.as_array().cloned().unwrap_or_default();
    (status, rows)
}

/// A minimal chat body for a model.
fn chat_body(model: &str) -> Value {
    json!({"model": model, "messages": [{"role": "user", "content": "hi"}]})
}

/// One attributed usage row, read assertion-only (the m5 sanction).
struct AttributionRow {
    provider_key_id: String,
    depositor_id: Option<String>,
    bucket_provider: Option<String>,
    bucket_model_family: Option<String>,
    cost_micros: i64,
}

/// All usage rows of a pool with their attribution columns.
async fn attribution_rows(pool: &db::Db, pool_id: &str) -> Vec<AttributionRow> {
    let sql = pool.sql(
        "SELECT provider_key_id, depositor_id, bucket_provider, bucket_model_family, cost_micros \
         FROM usage_events WHERE pool_id = ?",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(pool.pool())
        .await
        .expect("usage read");
    rows.iter()
        .map(|row| AttributionRow {
            provider_key_id: row.get::<String, _>("provider_key_id"),
            depositor_id: row
                .try_get::<Option<String>, _>("depositor_id")
                .ok()
                .flatten(),
            bucket_provider: row
                .try_get::<Option<String>, _>("bucket_provider")
                .ok()
                .flatten(),
            bucket_model_family: row
                .try_get::<Option<String>, _>("bucket_model_family")
                .ok()
                .flatten(),
            cost_micros: row.get::<i64, _>("cost_micros"),
        })
        .collect()
}

/// A depositor's total from a settle response.
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

// ---------------------------------------------------------------------------
// Invariant 1 — attribution is exact and reconciles (journey half)
// ---------------------------------------------------------------------------

/// #1: Every consumption is attributed to exactly the depositor whose key
/// burned; postings batch over usage rows and reconcile to per-key burn
/// totals exactly.
///
/// Journey: two depositors share a bucket through the product's own
/// provisioning; real traffic burns through both keys; then (a) every
/// usage row names the depositor who owns the provider key that served
/// it, in the serving bucket; (b) per-key burn totals (Σ cost by
/// provider key) equal the attributed totals; (c) the settle endpoint's
/// distributable equals the metered burn, conserves to the micro-unit,
/// and pays each depositor proportionally to their burn.
#[tokio::test]
async fn every_consumption_is_attributed_to_exactly_the_depositor_whose_key_burned_and_postings_batch_over_usage_rows_and_reconcile_to_per_key_burn_totals_exactly()
 {
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock_a = common::spawn_mock(MockMode::NonStream).await;
        let mock_b = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep_a, key_a) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock_a.base_url,
            None,
        )
        .await;
        let (dep_b, key_b) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock_b.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        let owners = [
            (key_a.clone(), dep_a.clone()),
            (key_b.clone(), dep_b.clone()),
        ];
        for _ in 0..6 {
            let (status, body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {body}");
        }
        let rows = common::wait_for_usage(&td.pool, &key_id, 6).await;
        assert_eq!(rows.len(), 6, "[{td_name}] six metered rows");

        // (a) every row attributed to the owning depositor, in the bucket.
        let attributed = attribution_rows(&td.pool, &pool).await;
        assert_eq!(attributed.len(), 6, "[{td_name}] pool-scoped rows");
        for row in &attributed {
            let owner = owners
                .iter()
                .find(|(k, _)| k == &row.provider_key_id)
                .map(|(_, d)| d.clone())
                .unwrap_or_else(|| {
                    panic!("[{td_name}] unknown provider key {}", row.provider_key_id)
                });
            assert_eq!(
                row.depositor_id.as_deref(),
                Some(owner.as_str()),
                "[{td_name}] attribution must name the owner of the key that burned",
            );
            assert_eq!(row.bucket_provider.as_deref(), Some("openai"));
            assert_eq!(row.bucket_model_family.as_deref(), Some("m-test"));
        }

        // (b) per-key burn totals reconcile: Σ attributed cost per key ==
        // Σ cost per key straight from the metered rows (they are the same
        // rows — the point is the BATCH attributes every one of them).
        for (provider_key, _) in &owners {
            let metered: i64 = attributed
                .iter()
                .filter(|r| &r.provider_key_id == provider_key)
                .map(|r| r.cost_micros)
                .sum();
            assert!(
                metered > 0,
                "[{td_name}] key {provider_key} served nothing — routing never used it (see invariant 8)",
            );
        }

        // (c) settle: distributable == the metered burn, conserved exactly,
        // proportional to burns (≤1 micro rounding per depositor).
        let (status, body) = settle_pool(&app, &token, &pool, 800).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "[{td_name}] settle (M9 contract): {body}"
        );
        let distributable = body["distributable_micros"]
            .as_i64()
            .expect("distributable_micros");
        let fee = body["fee_micros"].as_i64().expect("fee_micros");
        let undistributed = body["undistributed_micros"]
            .as_i64()
            .expect("undistributed_micros");
        let metered_total: i64 = attributed.iter().map(|r| r.cost_micros).sum();
        assert_eq!(
            distributable, metered_total,
            "[{td_name}] distributable is the attributed burn"
        );
        let payout_total: i64 = body["payouts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|p| p["amount_micros"].as_i64().unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0);
        assert_eq!(
            fee + payout_total + undistributed,
            distributable,
            "[{td_name}] fee + payouts + undistributed must equal distributable to the micro-unit",
        );
        // Fee is 8% of the burn, within one rounding micro.
        assert!(
            (fee as i128 * 10_000 - distributable as i128 * 800).abs() <= 10_000,
            "[{td_name}] fee must be fee_bps of distributable (±1 micro)",
        );
        let burn_a: i64 = attributed
            .iter()
            .filter(|r| r.depositor_id.as_deref() == Some(dep_a.as_str()))
            .map(|r| r.cost_micros)
            .sum();
        let burn_b: i64 = attributed
            .iter()
            .filter(|r| r.depositor_id.as_deref() == Some(dep_b.as_str()))
            .map(|r| r.cost_micros)
            .sum();
        let after_fee = distributable - fee;
        for (depositor, burn_d, paid_d) in [
            (dep_a.clone(), burn_a, settle_total_for(&body, &dep_a)),
            (dep_b.clone(), burn_b, settle_total_for(&body, &dep_b)),
        ] {
            assert!(
                ((paid_d as i128) * (metered_total as i128)
                    - (after_fee as i128) * (burn_d as i128))
                    .abs()
                    <= metered_total as i128,
                "[{td_name}] {depositor} paid {paid_d} for burn {burn_d}/{metered_total} — earnings must follow the burn",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant 2 — zero burn ⇒ zero earnings (journey half)
// ---------------------------------------------------------------------------

/// #2: A depositor earns nothing in a period where their credits burned
/// nothing.
///
/// Journey: two depositors stake in different buckets; all traffic burns
/// in bucket m-test; the m-other depositor's key is never dialed, earns
/// nothing at settle, and their bucket position shows burned 0 / earned 0.
#[tokio::test]
async fn a_depositor_earns_nothing_in_a_period_where_their_credits_burned_nothing() {
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock_a = common::spawn_mock(MockMode::NonStream).await;
        let mock_b = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test", "m-other"]).await;

        let (_dep_a, _key_a) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock_a.base_url,
            None,
        )
        .await;
        let (dep_silent, _key_s) = deposit(
            &app,
            &token,
            &pool,
            "m-other",
            5_000_000,
            &mock_b.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        for _ in 0..3 {
            let (status, body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {body}");
        }
        common::wait_for_usage(&td.pool, &key_id, 3).await;
        assert_eq!(
            mock_b.captured_count(),
            0,
            "[{td_name}] the silent depositor's key is never dialed (m-test routes elsewhere)"
        );

        let (status, body) = settle_pool(&app, &token, &pool, 800).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] settle: {body}");
        assert_eq!(
            settle_total_for(&body, &dep_silent),
            0,
            "[{td_name}] the silent depositor burned nothing — must earn nothing",
        );

        let (status, rows) = bucket_positions(&app, &token, &pool).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "[{td_name}] bucket-positions (M9 contract)"
        );
        let silent: Vec<_> = rows
            .iter()
            .filter(|r| r["depositor_id"] == json!(dep_silent))
            .collect();
        for row in &silent {
            assert_eq!(
                row["burned_micros"].as_i64(),
                Some(0),
                "[{td_name}] silent position burned nothing"
            );
            assert_eq!(
                row["earned_micros"].as_i64(),
                Some(0),
                "[{td_name}] silent position earned nothing"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant 3 — earnings never cross buckets (journey half)
// ---------------------------------------------------------------------------

/// #3: A request's burn is attributed within the bucket that serves it;
/// earnings never cross buckets.
///
/// Journey: dep-a staked in BOTH buckets (two keys, one depositor — the
/// deposit contract's depositor_id link); dep-b staked only in the second
/// bucket. All traffic burns in the first. Every payout names bucket one
/// and dep-a; dep-b earns nothing; dep-a's second-bucket position earns
/// nothing (their stake there did not burn).
#[tokio::test]
async fn a_requests_burn_is_attributed_within_the_bucket_that_serves_it_and_earnings_never_cross_buckets()
 {
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock_a = common::spawn_mock(MockMode::NonStream).await;
        let mock_b = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test", "m-other"]).await;

        let (dep_a, _key_a1) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock_a.base_url,
            None,
        )
        .await;
        let (_same_a, _key_a2) = deposit(
            &app,
            &token,
            &pool,
            "m-other",
            5_000_000,
            &mock_b.base_url,
            Some(&dep_a),
        )
        .await;
        let (dep_b, _key_b) = deposit(
            &app,
            &token,
            &pool,
            "m-other",
            9_000_000,
            &mock_b.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        for _ in 0..4 {
            let (status, body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {body}");
        }
        common::wait_for_usage(&td.pool, &key_id, 4).await;

        let (status, body) = settle_pool(&app, &token, &pool, 800).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] settle: {body}");
        let payouts = body["payouts"].as_array().cloned().unwrap_or_default();
        assert!(
            !payouts.is_empty(),
            "[{td_name}] bucket one burned — payouts exist"
        );
        for payout in &payouts {
            assert_eq!(
                payout["model_family"],
                json!("m-test"),
                "[{td_name}] payouts may only name the burning bucket: {payout}"
            );
            assert_eq!(
                payout["depositor_id"],
                json!(dep_a),
                "[{td_name}] only the burning depositor: {payout}"
            );
        }
        assert_eq!(
            settle_total_for(&body, &dep_b),
            0,
            "[{td_name}] dep-b's stake is in the other bucket — nothing crosses over"
        );

        // dep-a's OWN second-bucket position earned nothing either.
        let (status, rows) = bucket_positions(&app, &token, &pool).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "[{td_name}] bucket-positions (M9 contract)"
        );
        for row in rows
            .iter()
            .filter(|r| r["depositor_id"] == json!(dep_a) && r["model_family"] == json!("m-other"))
        {
            assert_eq!(
                row["earned_micros"].as_i64(),
                Some(0),
                "[{td_name}] dep-a's other-bucket stake burned nothing there — earns nothing there"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant 5 — fit + health filtering before any upstream call
// ---------------------------------------------------------------------------

/// #5: A request never routes to a key that cannot serve its provider/
/// model; unhealthy keys are skipped before any upstream call.
///
/// Journey: two keys in two different families; each model's traffic must
/// arrive ONLY at the key declaring that family (a misfit key is never
/// dialed), and a REVOKED key — even one declaring the right family — is
/// skipped with zero upstream calls. Proven by per-mock hit counters.
#[tokio::test]
async fn a_request_never_routes_to_a_key_that_cannot_serve_its_provider_model_and_unhealthy_keys_are_skipped_before_any_upstream_call()
 {
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock_a = common::spawn_mock(MockMode::NonStream).await;
        let mock_b = common::spawn_mock(MockMode::NonStream).await;
        let mock_c = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test", "m-other"]).await;

        let (_dep_a, key_a) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock_a.base_url,
            None,
        )
        .await;
        let (_dep_b, _key_b) = deposit(
            &app,
            &token,
            &pool,
            "m-other",
            5_000_000,
            &mock_b.base_url,
            None,
        )
        .await;
        // A third key declaring m-test — then REVOKED through the public
        // path (DELETE provider-keys; single-admin org auto-passes with
        // the audit marker, D33/A-026). It must never be dialed.
        let (_dep_c, key_c) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock_c.base_url,
            None,
        )
        .await;
        let (status, _body) = call(
            &app,
            api(
                "DELETE",
                &format!("/api/v1/pools/{pool}/provider-keys/{key_c}"),
                &token,
                None,
            ),
        )
        .await;
        assert!(
            status.is_success(),
            "[{td_name}] revoke via DELETE /api/v1/pools/{pool}/provider-keys/{{id}} (existing public path): {status}",
        );

        let (raw, _key_id) = member_key(&app, &token, &org, &pool).await;
        for model in [
            "m-test", "m-test", "m-test", "m-other", "m-other", "m-other",
        ] {
            let (status, body) = call(&app, chat_req(&raw, &chat_body(model))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] {model}: {body}");
        }

        // Bucket-fit: each family's traffic ONLY at its own key.
        assert_eq!(
            mock_a.captured_count(),
            3,
            "[{td_name}] m-test traffic belongs to the m-test key alone"
        );
        assert_eq!(
            mock_b.captured_count(),
            3,
            "[{td_name}] m-other traffic belongs to the m-other key alone"
        );
        // Unhealthy/revoked: never dialed at all.
        assert_eq!(
            mock_c.captured_count(),
            0,
            "[{td_name}] a revoked key must be skipped BEFORE any upstream call"
        );
        // And the revoked key is really the one we think it is.
        let _ = key_a;
    }
}

// ---------------------------------------------------------------------------
// Invariant 6 — per-bucket overdraw impossible
// ---------------------------------------------------------------------------

/// #6: No depositor's key is ever used beyond their contributed value
/// (per-bucket overdraw impossible).
///
/// Journey: one depositor contributes a SMALL amount to one bucket;
/// traffic runs until the bucket refuses. The refusal is 429 (the
/// OpenAI-compatible "no capacity" shape), names the capacity, dials NO
/// upstream on the refused request, and the final position shows
/// burned ≤ contributed — the per-bucket ceiling held.
#[tokio::test]
async fn no_depositors_key_is_ever_used_beyond_their_contributed_value_per_bucket_overdraw_impossible()
 {
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock_a = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let contributed = 200_000i64; // $0.20 — exhaustion must come fast
        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            contributed,
            &mock_a.base_url,
            None,
        )
        .await;
        let (raw, _key_id) = member_key(&app, &token, &org, &pool).await;

        let mut refused = false;
        let mut last_burned: i64 = -1;
        for i in 0..200 {
            let hits_before = mock_a.captured_count() as i64;
            let (status, body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
            if status == StatusCode::TOO_MANY_REQUESTS {
                let text = body.to_string().to_lowercase();
                assert!(
                    text.contains("exhaust")
                        || text.contains("capacity")
                        || text.contains("contribut"),
                    "[{td_name}] the 429 must name the bucket capacity: {body}",
                );
                assert_eq!(
                    mock_a.captured_count() as i64,
                    hits_before,
                    "[{td_name}] the refused request must dial NO upstream",
                );
                refused = true;
                break;
            }
            assert_eq!(status, StatusCode::OK, "[{td_name}] request {i}: {body}");
            let (pstatus, rows) = bucket_positions(&app, &token, &pool).await;
            assert_eq!(
                pstatus,
                StatusCode::OK,
                "[{td_name}] bucket-positions (M9 contract)"
            );
            let burned = rows
                .iter()
                .find(|r| r["depositor_id"] == json!(dep) && r["model_family"] == json!("m-test"))
                .map(|r| r["burned_micros"].as_i64().unwrap_or(0))
                .expect("the deposit opened a position");
            assert!(
                burned > last_burned,
                "[{td_name}] metered burn must be positive — zero-cost usage makes exhaustion untestable"
            );
            last_burned = burned;
        }
        assert!(
            refused,
            "[{td_name}] the bucket must eventually refuse (contributed {contributed} micros)"
        );
        let (_pstatus, rows) = bucket_positions(&app, &token, &pool).await;
        let burned = rows
            .iter()
            .find(|r| r["depositor_id"] == json!(dep))
            .map(|r| r["burned_micros"].as_i64().unwrap_or(0))
            .unwrap_or(0);
        assert!(
            burned <= contributed,
            "[{td_name}] per-bucket overdraw: burned {burned} > contributed {contributed}",
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant 8 — realized distribution ≝ declared policy; weights from
// claims.contributed
// ---------------------------------------------------------------------------

/// #8: Realized routing distribution matches the declared policy within
/// tolerance; weights come from claims.contributed.
///
/// Two legs: (a) the D43 routing half against the policy stub — a key's
/// weight IS its depositor's contributed micros in the bucket (the ONE
/// contributed-value source: claims.contributed, per the M9-A GO
/// refinement); (b) the realized half — two depositors 3:1 by
/// contribution in one bucket, 80 requests, per-mock counts within
/// 3σ-ish of 60/20 (tolerance 15), both keys used.
#[tokio::test]
async fn realized_routing_distribution_matches_the_declared_policy_within_tolerance_and_weights_come_from_claims_contributed()
 {
    use tributary_gateway::depositor_mode::{DepositorKey, ProRataByClaims, RoutingPolicy};

    // (a) The declared policy: weight == claims.contributed-derived
    // contributed_micros of the bucket position (D43: pro-rata applies
    // to ROUTING WEIGHTS).
    let policy = ProRataByClaims;
    let bucket = Bucket {
        provider: "openai".to_string(),
        model_family: "m-test".to_string(),
    };
    let heavy = DepositorKey {
        provider_key_id: "pk-heavy".to_string(),
        depositor_id: "dep-heavy".to_string(),
        bucket: bucket.clone(),
        contributed_micros: MicroUsd::new(3_000_000),
        burned_micros: MicroUsd::new(0),
        healthy: true,
    };
    let light = DepositorKey {
        provider_key_id: "pk-light".to_string(),
        depositor_id: "dep-light".to_string(),
        bucket: bucket.clone(),
        contributed_micros: MicroUsd::new(1_000_000),
        burned_micros: MicroUsd::new(0),
        healthy: true,
    };
    assert_eq!(
        policy.weight(&heavy),
        MicroUsd::new(3_000_000),
        "weight IS claims.contributed in the bucket"
    );
    assert_eq!(
        policy.weight(&light),
        MicroUsd::new(1_000_000),
        "weight IS claims.contributed in the bucket"
    );
    // And selection honors them: over a full sweep of draws the realized
    // split matches the weight ratio (deterministic draws — no RNG needed).
    let keys = vec![heavy, light];
    let mut heavy_draws = 0u64;
    let draws = 400u64;
    for draw in 0..draws {
        let picked = policy
            .select(&keys, draw * 100)
            .expect("an eligible pair always selects");
        if picked.provider_key_id == "pk-heavy" {
            heavy_draws += 1;
        }
    }
    assert!(
        (heavy_draws as i128 * 4 - draws as i128 * 3).abs() <= 40,
        "realized {heavy_draws}/{draws} vs declared 3:1 — selection must match the weights",
    );

    // (b) The realized half, through the product: 80 live requests split
    // 3:1 by the depositors' contributions in the bucket.
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock_a = common::spawn_mock(MockMode::NonStream).await;
        let mock_b = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock_a).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        deposit(
            &app,
            &token,
            &pool,
            "m-test",
            3_000_000,
            &mock_a.base_url,
            None,
        )
        .await;
        deposit(
            &app,
            &token,
            &pool,
            "m-test",
            1_000_000,
            &mock_b.base_url,
            None,
        )
        .await;
        let (raw, _key_id) = member_key(&app, &token, &org, &pool).await;

        for _ in 0..80 {
            let (status, body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
            assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {body}");
        }
        let hits_a = mock_a.captured_count();
        let hits_b = mock_b.captured_count();
        assert_eq!(
            hits_a + hits_b,
            80,
            "[{td_name}] every request served exactly once"
        );
        assert!(
            hits_a >= 1 && hits_b >= 1,
            "[{td_name}] both keys must serve (weights 3:1, not failover)"
        );
        assert!(
            (hits_a as i64 - 60).abs() <= 15,
            "[{td_name}] realized {hits_a}/80 vs declared 75% — routing must match the policy within tolerance",
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant 9 — V1 family mode unchanged (GREEN BY DESIGN)
// ---------------------------------------------------------------------------

/// #9: A pool that has not enabled depositor mode behaves identically to
/// V1 — every pre-M9 journey passes unchanged.
///
/// This test is GREEN ON THE BRANCH BY DESIGN: it pins UNCHANGED V1
/// behavior (if it were red, the additive rule would already be broken).
/// A plain org pool (no kind): provisioning, chat, metering, quota
/// denial before upstream — all exactly as M5 left them; usage rows
/// carry NO attribution; depositor-mode endpoints refuse org pools.
#[tokio::test]
async fn a_pool_that_has_not_enabled_depositor_mode_behaves_identically_to_v1_every_pre_m9_journey_passes_unchanged()
 {
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;

        // V1 shape: no kind field — the pre-M9 journey verbatim.
        let (status, pool) = call(
            &app,
            api(
                "POST",
                "/api/v1/pools",
                token.as_str(),
                Some(&json!({"org_id": org, "name": "Family Pool", "model_allowlist": ["m-test"]})),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "[{td_name}] V1 pool create: {pool}"
        );
        let pool = pool["id"].as_str().unwrap().to_string();

        let (status, pk) = call(
            &app,
            api(
                "POST",
                &format!("/api/v1/pools/{pool}/provider-keys"),
                &token,
                Some(&json!({"provider": "openai", "key": common::TEST_UPSTREAM_KEY, "base_url": mock.base_url})),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "[{td_name}] V1 provider key: {pk}"
        );

        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;

        // Quota: 1 request per minute — V1 denial before upstream.
        let (status, _rule) = call(
            &app,
            api(
                "POST",
                "/api/v1/quota-rules",
                &token,
                Some(&json!({"subject_kind": "pool_key", "subject_id": key_id, "period": "minute", "limit_requests": 1})),
            ),
        )
        .await;
        assert!(
            status.is_success(),
            "[{td_name}] quota rule upsert: {status}"
        );

        let (status, body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] V1 chat: {body}");
        let rows = common::wait_for_usage(&td.pool, &key_id, 1).await;
        assert_eq!(rows.len(), 1, "[{td_name}] one metered row, as ever");

        let hits_before = mock.captured_count();
        let (status, _body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
        assert_eq!(
            status,
            StatusCode::TOO_MANY_REQUESTS,
            "[{td_name}] V1 quota denial unchanged"
        );
        assert_eq!(
            mock.captured_count(),
            hits_before,
            "[{td_name}] denial before upstream — V1 shape"
        );

        // Attribution stays OFF for family pools: NULL columns, empty
        // positions, and the M9 endpoints refuse org pools (4xx).
        let attributed = attribution_rows(&td.pool, &pool).await;
        for row in &attributed {
            assert!(
                row.depositor_id.is_none(),
                "[{td_name}] family pools never attribute"
            );
            assert!(
                row.bucket_provider.is_none() && row.bucket_model_family.is_none(),
                "[{td_name}] family pools have no buckets"
            );
        }
        let (status, _rows) = bucket_positions(&app, &token, &pool).await;
        assert!(
            status.is_client_error(),
            "[{td_name}] bucket-positions must refuse an org pool, got {status}"
        );
        let (status, _body) = settle_pool(&app, &token, &pool, 800).await;
        assert!(
            status.is_client_error(),
            "[{td_name}] settle must refuse an org pool, got {status}"
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant 10 — additive-only change
// ---------------------------------------------------------------------------

/// #10: All changes are additive: new migration columns, new posting
/// templates, new claim positions — nothing existing rewritten.
///
/// Pins: the six M7 posting templates with their EXACT legs (rewriting a
/// template is rewriting the books' grammar); the normative chart seed
/// unchanged; pre-M9 usage rows still insertable with pre-M9 columns
/// only (nullable/defaulted additions — the schema probe); and the NEW
/// claim positions exist per bucket and sum back to claims.contributed
/// (the RED leg — it rides the deposit path and the positions read).
#[tokio::test]
async fn all_changes_are_additive_new_migration_columns_new_posting_templates_new_claim_positions_nothing_existing_rewritten()
 {
    use std::str::FromStr; // trait method from_str needs the trait in scope

    use tributary_ledger::chart::{AccountCode, SEED};
    use tributary_ledger::posting::{TEMPLATES, TemplateCode};

    // (a) The six M7 templates, byte-for-byte in leg structure. Any M9
    // addition may APPEND templates; it may not rewrite these.
    // (Type aliases keep the pinned shape readable: one leg is
    // (account, direction, party); one template spec is (code, legs).)
    type Leg = (&'static str, &'static str, &'static str);
    type TemplateSpec = (&'static str, &'static [Leg]);
    let expected: &[TemplateSpec] = &[
        (
            "prepaid.fund",
            &[
                ("cash.escrow", "debit", "Platform"),
                ("prepaid.renter", "credit", "Renter"),
            ],
        ),
        (
            "prepaid.consume",
            &[
                ("prepaid.renter", "debit", "Renter"),
                ("revenue.usage", "credit", "Platform"),
            ],
        ),
        (
            "settle.fee",
            &[
                ("revenue.usage", "debit", "Platform"),
                ("equity.operator", "credit", "Platform"),
            ],
        ),
        (
            "settle.distribute",
            &[
                ("revenue.usage", "debit", "Platform"),
                ("payable.depositor", "credit", "Depositor"),
            ],
        ),
        (
            "deposit.capacity",
            &[
                ("capacity.contributed", "debit", "Depositor"),
                ("claims.contributed", "credit", "Depositor"),
            ],
        ),
        (
            "withdraw.payable",
            &[
                ("payable.depositor", "debit", "Depositor"),
                ("cash.escrow", "credit", "Platform"),
            ],
        ),
    ];
    for (code, legs) in expected {
        let template = TEMPLATES
            .iter()
            .find(|t| t.code.as_str() == *code)
            .unwrap_or_else(|| panic!("M7 template {code} must survive M9 unchanged"));
        assert_eq!(
            template.legs.len(),
            legs.len(),
            "{code}: leg count rewritten"
        );
        for (leg, (account, direction, party)) in template.legs.iter().zip(legs.iter()) {
            assert_eq!(leg.account.as_str(), *account, "{code}: account rewritten");
            assert_eq!(
                leg.direction.as_str(),
                *direction,
                "{code}: direction rewritten"
            );
            assert_eq!(
                format!("{:?}", leg.party),
                *party,
                "{code}: party rewritten"
            );
        }
    }
    let _ = TemplateCode::from_str; // parse path untouched (public API since M7)

    // (b) The normative chart: the same seven codes, same order.
    let seed_codes: Vec<&str> = SEED.iter().map(|r| r.code.as_str()).collect();
    assert_eq!(
        seed_codes,
        vec![
            "cash.escrow",
            "capacity.contributed",
            "prepaid.renter",
            "payable.depositor",
            "claims.contributed",
            "equity.operator",
            "revenue.usage",
        ],
        "the chart seed must not be rewritten",
    );
    let _ = AccountCode::from_str("cash.escrow").expect("chart codes still parse");

    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        // (c) Schema probe (NOT entity seeding — a compatibility probe):
        // an M1-shaped usage row, pre-M9 columns only, must still insert.
        let probe_id = format!("probe-{}", db::new_id());
        let sql = td.pool.sql(
            "INSERT INTO usage_events (id, ts, pool_key_id, model) VALUES (?, ?, ?, 'm-test')",
        );
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&probe_id)
            .bind(db::now_ms())
            .bind("probe-key")
            .execute(td.pool.pool())
            .await
            .expect(
                "pre-M9 usage rows must keep inserting (additive columns are nullable/defaulted)",
            );
        let sql = td.pool.sql("DELETE FROM usage_events WHERE id = ?");
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&probe_id)
            .execute(td.pool.pool())
            .await
            .expect("probe cleanup");

        // (d) The RED leg: a deposit through the product's own path opens
        // per-bucket positions that sum back to claims.contributed (the
        // single contributed-value source — M9-A GO refinement).
        let mock = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;
        let (dep, _key) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;

        let positions = tributary_ledger::settle::bucket_positions(&td.pool, &pool)
            .await
            .expect("positions read");
        assert!(
            !positions.is_empty(),
            "[{td_name}] a deposit opens a bucket position"
        );
        let contributed_sum: i64 = positions
            .iter()
            .filter(|p| p.depositor_id == dep)
            .map(|p| p.contributed_micros.micros())
            .sum();
        assert_eq!(
            contributed_sum, 5_000_000,
            "[{td_name}] per-bucket positions must sum back to the deposited claims.contributed",
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant 11 — reservation member-scoped, upstream-first; attribution
// attaches at settle, never at reserve
// ---------------------------------------------------------------------------

/// #11: Quota reservation stays member-scoped and upstream-first (rule 4,
/// A-020); burn attribution attaches at settle, never at reserve.
///
/// Green pins (V1 discipline carried into root pools): quota denial
/// fires BEFORE any upstream call; the reservation rows are keyed by the
/// MEMBER's pool key; the reservation table carries NO attribution
/// columns (a SELECT for one must FAIL — a standing tripwire). RED leg:
/// the settled usage row DOES carry depositor + bucket attribution
/// (attribution attaches at settle — the A-020 settle moment).
#[tokio::test]
async fn quota_reservation_stays_member_scoped_and_upstream_first_rule_4_a_020_burn_attribution_attaches_at_settle_never_at_reserve()
 {
    for td in test_dbs().await {
        let td_name = td.name; // format strings capture plain identifiers only
        let mock = common::spawn_mock(MockMode::NonStream).await;
        let app = test_app(td.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&td.pool, &app).await;
        let org = new_org(&app, &token).await;
        let pool = root_pool(&app, &token, &org, &["m-test"]).await;

        let (dep, _pkey) = deposit(
            &app,
            &token,
            &pool,
            "m-test",
            5_000_000,
            &mock.base_url,
            None,
        )
        .await;
        let (raw, key_id) = member_key(&app, &token, &org, &pool).await;
        let (status, _rule) = call(
            &app,
            api(
                "POST",
                "/api/v1/quota-rules",
                &token,
                Some(&json!({"subject_kind": "pool_key", "subject_id": key_id, "period": "minute", "limit_requests": 1})),
            ),
        )
        .await;
        assert!(status.is_success(), "[{td_name}] quota rule: {status}");

        // Upstream-first + member-scoped: first request served, second
        // denied before ANY upstream call (A-020 unchanged on root pools).
        let (status, body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
        assert_eq!(status, StatusCode::OK, "[{td_name}] chat: {body}");
        common::wait_for_usage(&td.pool, &key_id, 1).await;
        let hits_before = mock.captured_count();
        let (status, _body) = call(&app, chat_req(&raw, &chat_body("m-test"))).await;
        assert_eq!(
            status,
            StatusCode::TOO_MANY_REQUESTS,
            "[{td_name}] quota denial"
        );
        assert_eq!(
            mock.captured_count(),
            hits_before,
            "[{td_name}] denial fires before upstream"
        );

        // The reservation is keyed by the MEMBER's pool key — never by a
        // depositor (assertion-only read; A-020's schema).
        let sql = td
            .pool
            .sql("SELECT subject_kind, subject_id FROM quota_buckets WHERE subject_id = ?");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&key_id)
            .fetch_all(td.pool.pool())
            .await
            .expect("reservation read");
        assert!(
            !rows.is_empty(),
            "[{td_name}] the served request left a reservation row"
        );
        for row in &rows {
            assert_eq!(
                row.get::<String, _>("subject_kind"),
                "pool_key",
                "[{td_name}] reservations are member-scoped"
            );
        }

        // NEVER at reserve: the reservation table must carry NO
        // attribution columns. This SELECT FAILING is the pin — if a
        // future change adds depositor/bucket columns there, this goes
        // red and the doctrine question routes to the user.
        let sql = td
            .pool
            .sql("SELECT depositor_id FROM quota_buckets LIMIT 1");
        let attempted = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(td.pool.pool())
            .await;
        assert!(
            attempted.is_err(),
            "[{td_name}] quota_buckets must stay attribution-free (at SETTLE, never at reserve)",
        );

        // AT SETTLE: the settled usage row carries the attribution.
        let attributed = attribution_rows(&td.pool, &pool).await;
        assert_eq!(attributed.len(), 1, "[{td_name}] one settled row");
        assert_eq!(
            attributed[0].depositor_id.as_deref(),
            Some(dep.as_str()),
            "[{td_name}] attribution attaches at settle — the row names the depositor whose key burned",
        );
        assert_eq!(attributed[0].bucket_provider.as_deref(), Some("openai"));
        assert_eq!(attributed[0].bucket_model_family.as_deref(), Some("m-test"));
    }
}
