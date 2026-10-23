//! M5 hardening journeys (director kickoff 2026-10-22) — the milestone's
//! done-when proofs, exercised through the PUBLIC API only (workflow step
//! 12): provisioning rides the control API, chat rides the data plane, and
//! only gateway-owned telemetry rows (IP history, usage) are seeded
//! directly — mirroring the M4 harness rule.
//!
//! Current residents:
//! - `j3` — D32 dedupe under a CONCURRENT burst (A-021): one flag, one
//!   email, one audit row, no matter how many requests race.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{MockMode, MockUpstream, chat_body, spawn_mock, test_app, test_dbs};
use serde_json::{Value, json};
use sqlx::Row;
use tributary_gateway::db;

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

/// A chat request with a specific `X-Forwarded-For` source (the D32 and
/// allowlist journeys simulate origin networks through it).
fn chat_req(key: &str, xff: Option<&str>, body: &Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {key}"));
    if let Some(ip) = xff {
        builder = builder.header("x-forwarded-for", ip);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// Same, with an explicit transport socket address — A-019's trust
/// decision needs the ACTUAL peer, which prod provides via
/// `into_make_service_with_connect_info` and tests provide here.
fn chat_req_from(
    key: &str,
    socket: std::net::SocketAddr,
    xff: Option<&str>,
    body: &Value,
) -> Request<Body> {
    let mut req = chat_req(key, xff, body);
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(socket));
    req
}

/// Polls until the predicate holds (detection writes land shortly after
/// the response; the gateway awaits its own audit write, not the flag).
async fn wait_for<F, Fut>(what: &str, mut pred: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if pred().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    panic!("condition never held: {what}");
}

/// Bootstrap host admin (direct create — the product's bootstrap path,
/// pre-login by design) + session login.
async fn bootstrap_admin_and_login(pool: &db::Db, app: &axum::Router) -> String {
    let email = format!("root-{}@tributary.dev", db::new_id());
    tributary_control::create_host_admin(pool, &email, "hunter2hunter2")
        .await
        .expect("bootstrap");
    let (status, body) = call(
        app,
        api(
            "POST",
            "/api/v1/auth/login",
            "",
            Some(&json!({"email": email, "password": "hunter2hunter2"})),
        ),
    )
    .await;
    // Empty bearer on the login call itself is fine (it carries the body),
    // but assert it worked.
    assert_eq!(status, StatusCode::OK, "host login: {body}");
    body["token"].as_str().unwrap().to_string()
}

/// A fully provisioned stack through the public API: org → pool → provider
/// key → member → issued member key. Returns ids + the raw member key.
struct M5Stack {
    #[allow(dead_code)] // org/pool ids become interesting to later journeys
    org_id: String,
    pool_id: String,
    key_id: String,
    key_raw: String,
}

async fn provision_stack(app: &axum::Router, token: &str, mock: &MockUpstream) -> M5Stack {
    let (status, org) = call(
        app,
        api(
            "POST",
            "/api/v1/orgs",
            token,
            Some(&json!({"name": "M5 Org", "slug": format!("m5-{}", db::new_id())})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "org: {org}");
    let org_id = org["id"].as_str().unwrap().to_string();

    let (status, pool) = call(
        app,
        api(
            "POST",
            "/api/v1/pools",
            token,
            Some(&json!({"org_id": org_id, "name": "M5 Pool"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "pool: {pool}");
    let pool_id = pool["id"].as_str().unwrap().to_string();

    let (status, _) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/provider-keys"),
            token,
            Some(&json!({
                "key": common::TEST_UPSTREAM_KEY,
                "base_url": mock.base_url,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "provider key");

    let member_email = format!("member-{}@trib.test", db::new_id());
    let (status, member) = call(
        app,
        api(
            "POST",
            "/api/v1/members",
            token,
            Some(&json!({
                "org_id": org_id,
                "email": member_email,
                "password": "hunter2hunter2",
            })),
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
    assert_eq!(status, StatusCode::CREATED, "key: {key}");
    M5Stack {
        org_id,
        pool_id,
        key_id: key["id"].as_str().unwrap().to_string(),
        key_raw: key["raw"].as_str().unwrap().to_string(),
    }
}

/// Seeds the per-key IP history the gateway owns (telemetry rows — no API
/// provisioning path exists, so direct seeding is sanctioned setup).
async fn seed_key_ip(pool: &db::Db, key_id: &str, ip: &str) {
    let sql = pool.sql(
        "INSERT INTO pool_key_ips (pool_key_id, ip, country, first_seen, last_seen) \
         VALUES (?, ?, NULL, ?, ?) \
         ON CONFLICT (pool_key_id, ip) DO UPDATE SET last_seen = excluded.last_seen",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(key_id)
        .bind(ip)
        .bind(db::now_ms())
        .bind(db::now_ms())
        .execute(pool.pool())
        .await
        .expect("seed key ip");
}

/// J3 — a concurrent two-IP incident raises EXACTLY one D32 flag and one
/// email (A-021: the partial unique index makes dedupe hold under race).
#[tokio::test]
async fn j3_d32_race_raises_exactly_one_flag_and_email() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // Pre-warm the key's active-source set with two shared networks so
        // EVERY concurrent request below sees networks ≥ 2 and races the
        // flag insert (the exact count-then-insert window A-021 closes).
        seed_key_ip(&test_db.pool, &stack.key_id, "91.207.174.9").await;
        seed_key_ip(&test_db.pool, &stack.key_id, "203.0.113.7").await;

        let body = chat_body();
        let ips = [
            "91.207.174.10",
            "203.0.113.8",
            "91.207.174.11",
            "203.0.113.9",
            "91.207.174.12",
            "203.0.113.10",
            "91.207.174.13",
            "203.0.113.11",
        ];
        let mut reqs = Vec::new();
        for ip in ips {
            reqs.push(call(&app, chat_req(&stack.key_raw, Some(ip), &body)));
        }
        let results = futures_util::future::join_all(reqs).await;
        for (i, (status, _)) in results.iter().enumerate() {
            assert_eq!(*status, StatusCode::OK, "D32 is alert-only (req {i})");
        }

        // Exactly one flag, one email, one audit row — regardless of how
        // many of the 8 racers reached the insert.
        wait_for("exactly one open d32 flag", || {
            let app = app.clone();
            let token = token.clone();
            let key_id = stack.key_id.clone();
            async move {
                let (_, flags) = call(
                    &app,
                    api("GET", "/api/v1/review-flags?status=open", &token, None),
                )
                .await;
                flags.as_array().is_some_and(|f| {
                    f.iter()
                        .filter(|r| {
                            r["kind"] == "distinct_ip_concurrency"
                                && r["pool_key_id"] == key_id.as_str()
                        })
                        .count()
                        == 1
                })
            }
        })
        .await;
        // The audit-row linkage needs the flag's id: fetch it after the
        // queue settles (deterministically exactly one now).
        let (_, flags) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let flag_id = flags
            .as_array()
            .expect("flag list")
            .iter()
            .filter(|r| {
                r["kind"] == "distinct_ip_concurrency" && r["pool_key_id"] == stack.key_id.as_str()
            })
            .map(|r| r["id"].as_str().expect("flag id").to_string())
            .next()
            .expect("the single d32 flag");

        // Email: exactly one outbox row for this org (no org_admin member
        // exists, so recipients fall back to the single host admin).
        let sql = test_db
            .pool
            .sql("SELECT COUNT(*) AS n FROM email_alerts WHERE org_id = ?");
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&stack.org_id)
            .fetch_one(test_db.pool.pool())
            .await
            .expect("outbox count")
            .get("n");
        assert_eq!(
            n, 1,
            "one sharing event, one alert email [{}]",
            test_db.name
        );

        // Audit: exactly one security.distinct_ip_concurrency row linked to
        // THE flag (target_id = flag id — the linkage the M4 suite pins).
        let sql = test_db.pool.sql(
            "SELECT COUNT(*) AS n FROM audit_log \
             WHERE action = 'security.distinct_ip_concurrency' AND target_id = ?",
        );
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&flag_id)
            .fetch_one(test_db.pool.pool())
            .await
            .expect("audit count")
            .get("n");
        assert_eq!(n, 1, "one audit row for the incident [{}]", test_db.name);
    }
}

/// A-022c (defense-in-depth): a pool_keys row whose `kind` is not `live`
/// must fail auth even with status='active' and no expiry. No product
/// path mints non-live kinds since D31 (canaries removed) — a row like
/// that means a manual insert, a bad restore, or a future regression;
/// the kind check is the backstop. The row is seeded directly because
/// the product deliberately has NO provisioning path for it.
#[tokio::test]
async fn a022c_non_live_key_kind_rejected() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // A "honey" key bound to the SAME healthy pool/user the journey
        // provisioned — isolating the kind check from everything else.
        let generated = tributary_gateway::keys::generate_key();
        let raw = generated.raw;
        let computed = generated.key_hash;
        db::insert_pool_key(
            &test_db.pool,
            &db::NewPoolKey {
                id: db::new_id(),
                key_hash: computed,
                key_prefix: "sk-pool-hon".into(),
                kind: "honey".into(),
                status: "active".into(),
                expires_at: None,
                user_id: None,
                pool_id: Some(stack.pool_id.clone()),
                created_at: db::now_ms(),
            },
        )
        .await
        .expect("seed honey key");

        let (status, body) = call(&app, chat_req(&raw, None, &chat_body())).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "non-live kind must fail auth [{}] — {body}",
            test_db.name
        );
    }
}

/// J1 — spoofed `X-Forwarded-For` cannot satisfy an IP allowlist pin
/// (A-019). Leg 1: with NO trusted proxy configured, only the socket
/// address counts — a header claiming a pinned network is ignored. Leg
/// 2: the real client of an unpinned network cannot smuggle itself into
/// the allowlist by forging a first hop. (The trusted-proxy POSITIVE
/// legs — rightmost-untrusted-hop parsing — join in the post-fix run.)
#[tokio::test]
async fn j1_spoofed_xff_cannot_pass_cidr_pin() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        // Pin the key to 10.0.0.0/8 (the office network, say).
        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/allowlists",
                    stack.pool_id, stack.key_id
                ),
                &token,
                Some(&json!({"cidr": "10.0.0.0/8"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "allowlist pin created");

        let attacker_socket = std::net::SocketAddr::from(([203, 0, 113, 99], 51000));

        // Leg 1 — no proxy configured: a spoofed header claiming the
        // pinned network MUST be ignored; the socket address (unpinned)
        // decides ⇒ 403.
        let (status, body) = call(
            &app,
            chat_req_from(
                &stack.key_raw,
                attacker_socket,
                Some("10.0.0.5"),
                &chat_body(),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "spoofed XFF must not satisfy the pin with no trusted proxy [{}] — {body}",
            test_db.name
        );

        // Control: the same socket WITHOUT any XFF also denies (the
        // socket really is the source when no proxy is trusted).
        let (status, _) = call(
            &app,
            chat_req_from(&stack.key_raw, attacker_socket, None, &chat_body()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "socket address decides");

        // Leg 2 — deep spoof: forged pinned hop + real attacker hop.
        // The RIGHTMOST untrusted hop (the attacker's real address) must
        // be what counts, never the forged left hop.
        let (status, body) = call(
            &app,
            chat_req_from(
                &stack.key_raw,
                attacker_socket,
                Some("10.0.0.5, 203.0.113.99"),
                &chat_body(),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "rightmost untrusted hop rules [{}] — {body}",
            test_db.name
        );
    }
}

/// J1 trusted-proxy legs (post-fix coverage — these need the new
/// `TRIBUTARY_TRUSTED_PROXIES` wiring): behind a CONFIGURED trusted
/// proxy, the rightmost untrusted hop is the client — a pinned-network
/// client passes, a spoofed first hop cannot smuggle an unpinned peer
/// in, and a fully-trusted chain falls back to the leftmost hop.
#[tokio::test]
async fn j1_trusted_proxy_rightmost_untrusted_hop_wins() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app =
            common::test_app_with_trusted(test_db.pool.clone(), &mock, &["127.0.0.1/32"]).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        let (status, _) = call(
            &app,
            api(
                "POST",
                &format!(
                    "/api/v1/pools/{}/keys/{}/allowlists",
                    stack.pool_id, stack.key_id
                ),
                &token,
                Some(&json!({"cidr": "10.0.0.0/8"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "pin created");

        // The proxy's own socket (127.0.0.1) is trusted; the client it
        // observed is 10.0.0.5 (rightmost untrusted hop) ⇒ allowed.
        let proxy_socket = std::net::SocketAddr::from(([127, 0, 0, 1], 60000));
        let (status, body) = call(
            &app,
            chat_req_from(&stack.key_raw, proxy_socket, Some("10.0.0.5"), &chat_body()),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "trusted proxy + pinned client passes [{}] — {body}",
            test_db.name
        );

        // Forged first hop + real attacker hop: the RIGHTMOST untrusted
        // hop (attacker, 198.51.100.7) decides ⇒ denied.
        let (status, body) = call(
            &app,
            chat_req_from(
                &stack.key_raw,
                proxy_socket,
                Some("10.0.0.5, 198.51.100.7"),
                &chat_body(),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "forged left hop must not smuggle an unpinned client [{}] — {body}",
            test_db.name
        );

        // Fully-trusted chain (client behind ANOTHER trusted proxy):
        // every hop trusted ⇒ leftmost (the origin the innermost proxy
        // recorded) is the client — 10.0.0.5 ⇒ allowed.
        let (status, body) = call(
            &app,
            chat_req_from(
                &stack.key_raw,
                proxy_socket,
                Some("10.0.0.5, 127.0.0.1"),
                &chat_body(),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "all-trusted chain falls back to leftmost hop [{}] — {body}",
            test_db.name
        );

        // Untrusted socket behind the SAME config: header ignored, socket
        // decides (127.0.0.1 trick does not survive an outside peer).
        let outside = std::net::SocketAddr::from(([203, 0, 113, 50], 51000));
        let (status, _) = call(
            &app,
            chat_req_from(&stack.key_raw, outside, Some("10.0.0.5"), &chat_body()),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "untrusted peer's header claims are worthless [{}]",
            test_db.name
        );
    }
}

/// J2 — a concurrent burst cannot exceed a quota cap by more than one
/// request's estimate (A-020: reserve→settle→release under race).
///
/// The cap is 60 tokens/day on the key; each request self-describes
/// `max_tokens: 18` plus a ~1-token prompt ⇒ estimate 19. Exactly three
/// fit (57 ≤ 60); from the FOURTH on, the reservation itself denies —
/// before the upstream call, which is the whole point: usage only lands
/// at response end, so pre-fix a burst fits N× through a cap sized for
/// one (the TOCTOU A-020 closes).
#[tokio::test]
async fn j2_concurrent_burst_cannot_exceed_cap() {
    for test_db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(test_db.pool.clone(), &mock).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_stack(&app, &token, &mock).await;

        let (status, body) = call(
            &app,
            api(
                "POST",
                "/api/v1/quota-rules",
                &token,
                Some(&json!({
                    "subject_kind": "pool_key",
                    "subject_id": stack.key_id,
                    "period": "day",
                    "limit_tokens": 60,
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "quota rule set: {body}");

        let body = json!({
            "model": "m-test",
            "max_tokens": 18,
            "messages": [{"role": "user", "content": "hi"}],
        });
        let mut reqs = Vec::new();
        for _ in 0..8 {
            reqs.push(call(&app, chat_req(&stack.key_raw, None, &body)));
        }
        let results = futures_util::future::join_all(reqs).await;
        let served = results.iter().filter(|(s, _)| *s == StatusCode::OK).count();
        let denied = results
            .iter()
            .filter(|(s, _)| *s == StatusCode::TOO_MANY_REQUESTS)
            .count();
        // Three fit (3 × 19 = 57 ≤ 60); five are denied BEFORE upstream.
        assert_eq!(
            served, 3,
            "cap 60, estimate 19 ⇒ exactly 3 concurrent admissions [{}]",
            test_db.name
        );
        assert_eq!(denied, 5, "the rest deny with 429 [{}]", test_db.name);

        // Served ACTUAL usage respects the cap (3 × 18 = 54 ≤ 60) — the
        // burst cannot exceed the cap by more than one request's estimate.
        common::wait_for_usage(&test_db.pool, &stack.key_id, 3).await;
        let sql = test_db.pool.sql(
            "SELECT CAST(COALESCE(SUM(prompt_tokens + completion_tokens), 0) AS BIGINT) AS t \
             FROM usage_events WHERE pool_key_id = ?",
        );
        let used: i64 = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&stack.key_id)
            .fetch_one(test_db.pool.pool())
            .await
            .expect("usage sum")
            .get("t");
        assert!(used <= 60 + 19, "usage {used} exceeds cap+one-estimate");

        // Settle drained the window's reservation back to zero.
        wait_for("reservation drains after settle", || {
            let pool = test_db.pool.clone();
            let key_id = stack.key_id.clone();
            async move {
                let sql = pool.sql(
                    "SELECT CAST(COALESCE(SUM(reserved_tokens), 0) AS BIGINT) AS r \
                     FROM quota_buckets WHERE subject_id = ?",
                );
                let r: i64 = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(&key_id)
                    .fetch_one(pool.pool())
                    .await
                    .expect("reserved sum")
                    .get("r");
                r == 0
            }
        })
        .await;
    }
}
