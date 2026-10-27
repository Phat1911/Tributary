//! M5.5 slice 2 — Gemini provider journeys (D36). The pool already routes
//! to whatever `base_url` a provider key carries and authenticates with
//! `Authorization: Bearer` (chat.rs), which is EXACTLY what Google's
//! OpenAI-COMPATIBLE endpoint speaks — so a Gemini key is a first-class
//! pool key with zero gateway changes. These journeys PROVE that through
//! the public API only (workflow step 12): provisioning rides the control
//! API, chat rides the data plane, and the mock below mirrors the real
//! endpoint's shape (`…/v1beta/openai/…`, Bearer auth, gemini-* models).
//!
//! Residents:
//! - `j_a` — a GEMINI-labeled key provisions encrypted + masked (rule 2).
//! - `j_b` — a member request for a gemini-* model routes to the compat
//!   endpoint, streams back, and meters exactly like an OpenAI key.
//! - `j_c_quota` — quota denial fires BEFORE the Gemini upstream is hit.
//! - `j_c_d32` — the D32 sharing flag fires on Gemini-routed traffic.
//!
//! Teaching note (why Bearer, never `?key=`): the NATIVE Gemini API
//! accepts the key as a `?key=` query parameter — but query strings land
//! in access logs, proxy logs, and browser history, which is precisely
//! how keys leak. The compat endpoint accepts the key as a header, so the
//! gateway's existing `bearer_auth` call is the only correct shape. The
//! mock below REJECTS any request with a query string so that a future
//! regression to `?key=` auth turns these tests red.

mod common;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use common::test_dbs;
use serde_json::{Value, json};
use sqlx::Row;
use std::sync::{Arc, Mutex};
use tributary_gateway::build_full_app;
use tributary_gateway::config::Config;
use tributary_gateway::db;

/// The upstream secret the mock accepts — AI-Studio keys really do start
/// with `AIza`, so the masked display form exercises the real shape.
pub const GEMINI_UPSTREAM_KEY: &str = "AIzaSySUPERSECRET-gemini-0123456789";

/// The model id the mock reports — mirrors the real endpoint's free-tier
/// flash default NAME SHAPE. Model identities are config, never code
/// (config.rs); the test only proves a `gemini-*` string flows through.
pub const GEMINI_MODEL: &str = "gemini-3.8-flash";

// ---------------------------------------------------------------------------
// Gemini-compat mock upstream
// ---------------------------------------------------------------------------

/// One captured upstream hit: the Authorization header, the full request
/// URI, and the parsed body. The URI is captured so tests can pin the
/// "never a query string" rule (no `?key=` leakage).
#[derive(Debug, Clone)]
pub struct GeminiHit {
    pub auth: Option<String>,
    pub uri: String,
    pub body: Value,
}

#[derive(Clone)]
pub struct GeminiMock {
    /// The pool-facing base URL — deliberately shaped like the real
    /// compat endpoint (`…/v1beta/openai`), because the gateway appends
    /// `/chat/completions` and the control probe appends `/models`.
    pub base_url: String,
    pub hits: Arc<Mutex<Vec<GeminiHit>>>,
}

impl GeminiMock {
    /// How many requests reached the mock (the "denied before upstream"
    /// assertion for quota denials).
    pub fn hit_count(&self) -> usize {
        self.hits.lock().unwrap().len()
    }
}

/// Spawns the mock on an ephemeral port, serving the two routes the
/// product calls: `GET {base}/models` (the key-health probe AND the
/// model-enumeration shape J-E uses) and `POST {base}/chat/completions`
/// (the serving path). Correct Bearer → 200; wrong Bearer → 401; any
/// request carrying a query string → 401 (the `?key=` tripwire). Stream
/// vs JSON is decided by the REQUEST's `stream` field, exactly like the
/// real compat endpoint.
pub async fn spawn_gemini_compat() -> GeminiMock {
    let hits: Arc<Mutex<Vec<GeminiHit>>> = Arc::new(Mutex::new(Vec::new()));
    let state = hits.clone();

    let app = Router::new()
        .route(
            "/v1beta/openai/models",
            axum::routing::get(|headers: HeaderMap, uri: axum::http::Uri| async move {
                if uri.query().is_some() {
                    return gemini_reject();
                }
                if bearer_is_valid(&headers) {
                    (
                        StatusCode::OK,
                        Json(json!({
                            "object": "list",
                            "data": [
                                {"id": GEMINI_MODEL, "object": "model", "owned_by": "google"},
                                {"id": "gemini-3.8-pro", "object": "model", "owned_by": "google"}
                            ]
                        })),
                    )
                        .into_response()
                } else {
                    gemini_reject()
                }
            }),
        )
        .route(
            "/v1beta/openai/chat/completions",
            post(
                |State(hits): State<Arc<Mutex<Vec<GeminiHit>>>>,
                 headers: HeaderMap,
                 uri: axum::http::Uri,
                 body: Bytes| async move {
                    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    {
                        let mut guard = hits.lock().unwrap();
                        guard.push(GeminiHit {
                            auth: headers
                                .get(header::AUTHORIZATION)
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                            uri: uri.to_string(),
                            body: parsed.clone(),
                        });
                    }
                    if uri.query().is_some() {
                        return gemini_reject();
                    }
                    if !bearer_is_valid(&headers) {
                        return gemini_reject();
                    }
                    if parsed["stream"].as_bool().unwrap_or(false) {
                        gemini_sse()
                    } else {
                        Json(json!({
                            "id": "chatcmpl-gemini-mock",
                            "object": "chat.completion",
                            "model": GEMINI_MODEL,
                            "choices": [{
                                "index": 0,
                                "message": {"role": "assistant", "content": "Xin chào from the compat endpoint!"},
                                "finish_reason": "stop",
                            }],
                            // Same token counts as the OpenAI mock (11/7):
                            // J-C's quota arithmetic stays identical, which
                            // IS the provider-agnosticism proof.
                            "usage": {"prompt_tokens": 11, "completion_tokens": 7},
                        }))
                        .into_response()
                    }
                },
            ),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    GeminiMock {
        base_url: format!("http://{addr}/v1beta/openai"),
        hits,
    }
}

/// The mock's uniform 401 (bad key OR query-string auth attempt).
fn gemini_reject() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": {"message": "bad bearer", "type": "invalid_request_error"}})),
    )
        .into_response()
}

/// True when the Authorization header carries the mock's exact Bearer.
fn bearer_is_valid(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a == format!("Bearer {GEMINI_UPSTREAM_KEY}"))
}

/// The mock's SSE script — same chunk shape the OpenAI mock emits
/// (role delta, two content deltas, stop, usage chunk, [DONE]) so the
/// gateway's stream pass-through and usage-strip logic see an identical
/// wire format regardless of provider.
fn gemini_sse() -> Response {
    let chunk = |delta: &str| {
        format!(
            "data: {{\"id\":\"g1\",\"object\":\"chat.completion.chunk\",\"model\":\"{GEMINI_MODEL}\",\"choices\":[{{\"index\":0,\"delta\":{delta}}}]}}\n\n"
        )
    };
    let mut script = String::new();
    script.push_str(&chunk(r#"{"role":"assistant","content":"Xin"}"#));
    script.push_str(&chunk(r#"{"content":" chào"}"#));
    script.push_str(&format!(
        "data: {{\"id\":\"g1\",\"object\":\"chat.completion.chunk\",\"model\":\"{GEMINI_MODEL}\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n"
    ));
    script.push_str(&format!(
        "data: {{\"id\":\"g1\",\"object\":\"chat.completion.chunk\",\"model\":\"{GEMINI_MODEL}\",\"choices\":[],\"usage\":{{\"prompt_tokens\":9,\"completion_tokens\":3}}}}\n\n"
    ));
    script.push_str("data: [DONE]\n\n");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(script))
        .unwrap()
}

// ---------------------------------------------------------------------------
// App + provisioning helpers (public API only — workflow step 12)
// ---------------------------------------------------------------------------

/// Builds the merged app with NO Gemini-specific configuration — that is
/// the slice's thesis: the gateway is provider-agnostic by construction,
/// and this builder copying `common::test_app` (minus its unused mock
/// parameter) proves no new config knob was needed for Gemini.
async fn gemini_app(pool: db::Db) -> axum::Router {
    let config = Config {
        database_url: String::new(),
        master_key: common::test_master(),
        bind: String::new(),
        default_models: vec![GEMINI_MODEL.to_string()],
        log_json: false,
        smtp: None,
        geoip_db_path: None,
        trusted_proxies: vec![],
        static_dir: None,
    };
    build_full_app(pool, config).await.expect("build full app")
}

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

/// A chat request whose client IP arrives the way production delivers
/// it — the transport socket (A-019; headers only count from trusted
/// proxies, so tests attach the extension the resolver really reads).
fn chat_req(key: &str, ip: Option<&str>, body: &Value) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    if let Some(ip) = ip {
        let addr = std::net::SocketAddr::new(ip.parse().expect("bare IP literal"), 54321);
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(addr));
    }
    req
}

/// Polls until the predicate holds (detection writes land shortly after
/// the response returns; the gateway awaits its audit write, not the
/// flag insert).
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

/// Bootstrap host admin (the product's bootstrap path, pre-login by
/// design) + session login. Shared host across this binary's parallel
/// tests against one Postgres — the m5 harness pattern.
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

/// A fully provisioned GEMINI stack through the public API: org → pool
/// (gemini allowlist) → gemini-labeled provider key (compat base_url) →
/// member → issued member key.
struct GeminiStack {
    #[allow(dead_code)] // org scope becomes interesting to later journeys
    org_id: String,
    pool_id: String,
    provider_key_id: String,
    key_id: String,
    key_raw: String,
}

async fn provision_gemini_stack(app: &axum::Router, token: &str, mock: &GeminiMock) -> GeminiStack {
    let (status, org) = call(
        app,
        api(
            "POST",
            "/api/v1/orgs",
            token,
            Some(&json!({"name": "Gemini Org", "slug": format!("gem-{}", db::new_id())})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "org: {org}");
    let org_id = org["id"].as_str().unwrap().to_string();

    // The pool's allowlist names the gemini model — matching is a PLAIN
    // string equality (chat.rs), no gpt-* shape assumption anywhere.
    let (status, pool) = call(
        app,
        api(
            "POST",
            "/api/v1/pools",
            token,
            Some(&json!({
                "org_id": org_id,
                "name": "Gemini Pool",
                "model_allowlist": [GEMINI_MODEL]
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "pool: {pool}");
    let pool_id = pool["id"].as_str().unwrap().to_string();

    // J-A's provisioning leg: provider label "gemini" + the compat
    // endpoint base_url. The health probe hits {base}/models — the mock
    // accepts the Bearer and the key lands "ok".
    let (status, pk) = call(
        app,
        api(
            "POST",
            &format!("/api/v1/pools/{pool_id}/provider-keys"),
            token,
            Some(&json!({
                "provider": "gemini",
                "key": GEMINI_UPSTREAM_KEY,
                "base_url": mock.base_url,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "gemini provider key: {pk}");
    let provider_key_id = pk["id"].as_str().unwrap().to_string();

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
    assert_eq!(status, StatusCode::CREATED, "member key: {key}");
    GeminiStack {
        org_id,
        pool_id,
        provider_key_id,
        key_id: key["id"].as_str().unwrap().to_string(),
        key_raw: key["raw"].as_str().unwrap().to_string(),
    }
}

/// Polls until ≥ `min` usage rows exist for the key (streaming rows are
/// written by a background task after the last byte leaves).
async fn wait_for_usage(pool: &db::Db, key_id: &str, min: usize) -> Vec<db::UsageEventRow> {
    for _ in 0..100 {
        let rows = db::usage_events_for_key(pool, key_id)
            .await
            .expect("usage read");
        if rows.len() >= min {
            return rows;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    panic!("usage events for key {key_id} never reached {min}");
}

// ---------------------------------------------------------------------------
// J-A — provisioning
// ---------------------------------------------------------------------------

/// J-A: a host admin adds a GEMINI provider key through the public API;
/// it lands envelope-encrypted (ciphertext only) and every API surface
/// shows the masked form (rule 2) with the provider label intact.
#[tokio::test]
async fn j_a_gemini_key_provisioned_encrypted_and_masked() {
    for test_db in test_dbs().await {
        let mock = spawn_gemini_compat().await;
        let app = gemini_app(test_db.pool.clone()).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_gemini_stack(&app, &token, &mock).await;

        // Listing is the masked view: provider label present, prefix in
        // first-5…last-4 form, and NO secret material anywhere.
        let (status, list) = call(
            &app,
            api(
                "GET",
                &format!("/api/v1/pools/{}/provider-keys", stack.pool_id),
                &token,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "[{}]", test_db.name);
        let entry = list
            .as_array()
            .expect("key list")
            .iter()
            .find(|k| k["id"] == stack.provider_key_id.as_str())
            .expect("the gemini key");
        assert_eq!(entry["provider"], "gemini", "provider label round-trips");
        assert!(entry["key_prefix"].as_str().unwrap().contains('…'));
        let list_str = list.to_string();
        assert!(
            !list_str.contains(GEMINI_UPSTREAM_KEY),
            "no raw gemini key in API output"
        );
        assert!(!list_str.contains("ciphertext"), "no ciphertext field");
        assert!(!list_str.contains("dek_wrapped"), "no dek field");

        // At-rest truth (assertion-only read, the m5 sanction): the row
        // is sealed material bound to this row id, provider labeled
        // gemini, pointed at the compat endpoint.
        let rows = tributary_control::db::provider_keys_by_pool(&test_db.pool, &stack.pool_id)
            .await
            .expect("rows");
        let row = rows
            .iter()
            .find(|r| r.id == stack.provider_key_id)
            .expect("stored row");
        assert_ne!(row.ciphertext, GEMINI_UPSTREAM_KEY);
        assert!(!row.ciphertext.contains("SUPERSECRET"));
        assert!(!row.dek_wrapped.contains("SUPERSECRET"));
        assert_eq!(row.provider, "gemini");
        assert!(
            row.base_url
                .trim_end_matches('/')
                .ends_with("/v1beta/openai"),
            "compat endpoint stored verbatim: {}",
            row.base_url
        );
    }
}

// ---------------------------------------------------------------------------
// J-B — serving, metering, fingerprinting
// ---------------------------------------------------------------------------

/// J-B: a member key asks for the gemini model; the gateway swaps in the
/// GEMINI provider key (Bearer header, no query string), the compat
/// endpoint's answer streams back, and the metering row + IP fingerprint
/// land exactly as they would for an OpenAI-routed call.
#[tokio::test]
async fn j_b_gemini_route_serves_meters_and_fingerprints() {
    for test_db in test_dbs().await {
        let mock = spawn_gemini_compat().await;
        let app = gemini_app(test_db.pool.clone()).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_gemini_stack(&app, &token, &mock).await;

        // Non-stream leg.
        let body = json!({
            "model": GEMINI_MODEL,
            "messages": [{"role": "user", "content": "xin chào"}],
        });
        let (status, resp) = call(&app, chat_req(&stack.key_raw, None, &body)).await;
        assert_eq!(status, StatusCode::OK, "[{}] {resp}", test_db.name);
        assert_eq!(resp["model"], GEMINI_MODEL, "upstream model echoes");
        assert_eq!(
            resp["choices"][0]["message"]["content"],
            "Xin chào from the compat endpoint!"
        );

        // Stream leg: the member asked `stream` but NOT include_usage —
        // the gateway injects it upstream, meters from it, and STRIPS it
        // from what the member receives (provider-agnostic behavior).
        let sse_body = json!({
            "model": GEMINI_MODEL,
            "stream": true,
            "messages": [{"role": "user", "content": "stream xin chào"}],
        });
        let stream_req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", stack.key_raw))
            .body(Body::from(sse_body.to_string()))
            .unwrap();
        use tower::ServiceExt;
        let response = app.clone().oneshot(stream_req).await.expect("oneshot");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("stream body");
        let text = String::from_utf8_lossy(&bytes).to_string();
        assert!(
            text.contains("chat.completion.chunk"),
            "chunks passed through"
        );
        assert!(text.contains("[DONE]"), "stream terminated properly");
        assert!(
            !text.contains("\"usage\""),
            "injected usage chunk stripped downstream"
        );

        // Upstream truth: TWO hits, both Bearer-authed with the GEMINI
        // key, both on the compat path, NEITHER with a query string.
        wait_for("two gemini upstream hits", || {
            let mock = mock.clone();
            async move { mock.hit_count() >= 2 }
        })
        .await;
        {
            let hits = mock.hits.lock().unwrap();
            assert_eq!(hits.len(), 2);
            for hit in hits.iter() {
                assert_eq!(
                    hit.auth.as_deref(),
                    Some(format!("Bearer {GEMINI_UPSTREAM_KEY}").as_str()),
                    "provider key swapped in as a Bearer header"
                );
                assert!(
                    hit.uri.starts_with("/v1beta/openai/chat/completions"),
                    "routed to the compat path: {}",
                    hit.uri
                );
                assert!(
                    !hit.uri.contains('?'),
                    "never a query string (no ?key= leakage): {}",
                    hit.uri
                );
            }
        }

        // Metering truth: two usage rows on the gemini provider key,
        // model names carried through verbatim, tokens counted.
        let rows = wait_for_usage(&test_db.pool, &stack.key_id, 2).await;
        assert!(rows.iter().all(|r| r.model == GEMINI_MODEL));
        assert!(
            rows.iter()
                .all(|r| r.provider_key_id.as_deref() == Some(stack.provider_key_id.as_str()))
        );
        assert!(
            rows.iter()
                .any(|r| r.prompt_tokens == 11 && r.completion_tokens == 7)
        );
        assert!(
            rows.iter()
                .any(|r| r.prompt_tokens == 9 && r.completion_tokens == 3)
        );

        // IP fingerprint: the member's socket IP was recorded for the
        // key — the D32 sensor's input, identical for any provider
        // (gateway-owned telemetry row; read directly for assertion).
        let sql = test_db
            .pool
            .sql("SELECT COUNT(*) AS n FROM pool_key_ips WHERE pool_key_id = ?");
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&stack.key_id)
            .fetch_one(test_db.pool.pool())
            .await
            .expect("ip row count")
            .get("n");
        assert_eq!(
            n, 1,
            "member IP fingerprinted on gemini-routed traffic [{}]",
            test_db.name
        );
    }
}

// ---------------------------------------------------------------------------
// J-C — governance: quota denial + D32 flag on Gemini-routed traffic
// ---------------------------------------------------------------------------

/// J-C (quota leg): a day cap fires as a 429 BEFORE the Gemini upstream
/// is called, with the audit row the M4 contract requires — identical to
/// the OpenAI-routed j5 journey (the check runs at the gateway, upstream
/// of the provider difference by construction).
#[tokio::test]
async fn j_c_quota_denial_before_gemini_upstream() {
    for test_db in test_dbs().await {
        let mock = spawn_gemini_compat().await;
        let app = gemini_app(test_db.pool.clone()).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_gemini_stack(&app, &token, &mock).await;

        // Day cap tighter than one request's usage (11+7 = 18 tokens).
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

        // A-020 arithmetic (same as j5): declare max_tokens so the
        // first request's reservation (1 + 8) fits; its SETTLE (18)
        // exhausts the cap and the second request denies at admission.
        let body = json!({
            "model": GEMINI_MODEL, "max_tokens": 8,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let (status, _) = call(&app, chat_req(&stack.key_raw, None, &body)).await;
        assert_eq!(status, StatusCode::OK, "first request serves");
        let before = mock.hit_count();
        let (status, err) = call(&app, chat_req(&stack.key_raw, None, &body)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{err}");
        assert_eq!(mock.hit_count(), before, "denied before the gemini call");

        // The 429 is audited (key-scoped, awaited before the response).
        let (_, audit) = call(&app, api("GET", "/api/v1/audit?limit=200", &token, None)).await;
        let quota_rows =
            audit.as_array().expect("audit array").iter().filter(|r| {
                r["action"] == "security.quota_denied" && r["target_id"] == stack.key_id
            });
        assert_eq!(
            quota_rows.count(),
            1,
            "exactly one quota_denied row for THIS key [{}]",
            test_db.name
        );
    }
}

/// J-C (D32 leg): the same member key seen from two different /24s on
/// GEMINI-routed traffic raises the distinct_ip_concurrency flag —
/// alert-only, exactly once — proving the sharing tripwire is keyed on
/// the MEMBER key, not on which provider serves the tokens.
#[tokio::test]
async fn j_c_d32_flag_fires_on_gemini_traffic() {
    for test_db in test_dbs().await {
        let mock = spawn_gemini_compat().await;
        let app = gemini_app(test_db.pool.clone()).await;
        let token = bootstrap_admin_and_login(&test_db.pool, &app).await;
        let stack = provision_gemini_stack(&app, &token, &mock).await;

        let body = json!({
            "model": GEMINI_MODEL,
            "messages": [{"role": "user", "content": "hi"}],
        });

        // Same-/24 hosts are ONE source: no flag from these two.
        let (status, _) = call(&app, chat_req(&stack.key_raw, Some("91.207.174.9"), &body)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            &app,
            chat_req(&stack.key_raw, Some("91.207.174.254"), &body),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "same-/24 hosts are one source");

        // Second /24 → the flag fires; the request still succeeds.
        let (status, _) = call(&app, chat_req(&stack.key_raw, Some("203.0.113.7"), &body)).await;
        assert_eq!(status, StatusCode::OK, "D32 is alert-only, never blocks");

        wait_for("d32 flag on gemini-routed key", || {
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
                    f.iter().any(|r| {
                        r["kind"] == "distinct_ip_concurrency"
                            && r["pool_key_id"] == key_id.as_str()
                            && r["detail_json"]["shared_networks"]
                                .as_array()
                                .is_some_and(|n| n.len() == 2)
                    })
                })
            }
        })
        .await;

        // Exactly one flag for this key (dedupe held).
        let (_, flags) = call(
            &app,
            api("GET", "/api/v1/review-flags?status=open", &token, None),
        )
        .await;
        let count = flags
            .as_array()
            .expect("flags array")
            .iter()
            .filter(|r| {
                r["kind"] == "distinct_ip_concurrency" && r["pool_key_id"] == stack.key_id.as_str()
            })
            .count();
        assert_eq!(count, 1, "one sharing event, one flag [{}]", test_db.name);

        // And every chat that served was gemini-routed (the sensor ran
        // on provider-agnostic traffic).
        assert_eq!(
            mock.hit_count(),
            3,
            "all three requests hit the compat endpoint"
        );
    }
}
