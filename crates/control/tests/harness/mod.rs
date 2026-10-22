//! Shared test harness: dual-DB pools, mock upstream, control router, and
//! HTTP helpers. Never contacts a real provider.

// Each integration-test binary links this module but uses only a slice of
// the helpers — silence the per-binary "never used" noise.
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::routing::get;
use serde_json::Value;
use tributary_common::db::{Db, run_migrations};

/// The one bearer the mock upstream accepts (validation probe target).
pub const UPSTREAM_KEY: &str = "sk-upstream-SUPERSECRET-0123456789abcdef";

/// A database under test plus anything that must outlive the pool.
pub struct TestDb {
    pub name: &'static str,
    pub pool: Db,
    _keep_alive: Option<tempfile::TempDir>,
}

/// SQLite (tempfile) always; Postgres when TRIBUTARY_TEST_DATABASE_URL is set
/// (CI always sets it). Mirrors the gateway test harness.
pub async fn test_dbs() -> Vec<TestDb> {
    let mut dbs = vec![sqlite_db().await];
    match std::env::var("TRIBUTARY_TEST_DATABASE_URL") {
        Ok(url) => match Db::connect(&url).await {
            Ok(db) => {
                run_migrations(&db).await.expect("postgres migrations");
                dbs.push(TestDb {
                    name: "postgres",
                    pool: db,
                    _keep_alive: None,
                });
            }
            // PANIC, not skip (final-scan): a silent skip turns a
            // red dual-DB suite green SQLite-only — proven to hide a
            // mis-forwarded env var behind a 100%-pass run. Opt out
            // explicitly with TRIBUTARY_ALLOW_PG_SKIP=1.
            Err(err) => {
                if std::env::var("TRIBUTARY_ALLOW_PG_SKIP").is_ok() {
                    eprintln!("postgres tests skipped (allowed): {err:#}");
                } else {
                    panic!(
                        "TRIBUTARY_TEST_DATABASE_URL is set but Postgres connect \
                         failed — refusing to silently pass SQLite-only: {err:#}"
                    );
                }
            }
        },
        Err(_) => eprintln!("TRIBUTARY_TEST_DATABASE_URL unset — Postgres half skipped"),
    }
    dbs
}

/// Fresh migrated SQLite: real tempfile on Linux/CI; single-connection
/// in-memory on the Windows cargo-interop (A-002: the Any driver's URL form
/// cannot express absolute Windows paths).
async fn sqlite_db() -> TestDb {
    #[cfg(not(windows))]
    {
        use sqlx::ConnectOptions;
        use sqlx::sqlite::SqliteConnectOptions;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("control-test.db");
        std::fs::File::create(&file).expect("pre-create sqlite file");
        let url = SqliteConnectOptions::new()
            .filename(&file)
            .to_url_lossy()
            .to_string();
        let pool = Db::connect(&url).await.expect("sqlite connect");
        run_migrations(&pool).await.expect("sqlite migrations");
        TestDb {
            name: "sqlite",
            pool,
            _keep_alive: Some(dir),
        }
    }
    #[cfg(windows)]
    {
        let pool = tributary_common::db::connect_sqlite_memory()
            .await
            .expect("sqlite memory");
        run_migrations(&pool).await.expect("sqlite migrations");
        TestDb {
            name: "sqlite",
            pool,
            _keep_alive: None,
        }
    }
}

/// A mock provider that answers `GET /models` like OpenAI would: 200 with a
/// model list for the correct bearer, 401 otherwise. This is exactly the
/// endpoint the key-health probe calls.
pub struct MockUpstream {
    pub base_url: String,
}

/// Spawns the mock on an ephemeral localhost port.
pub async fn spawn_models_mock() -> MockUpstream {
    let app = Router::new().route(
        "/models",
        get(|State(()): State<()>, headers: HeaderMap| async move {
            let auth = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            if auth == format!("Bearer {UPSTREAM_KEY}") {
                (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "object": "list",
                        "data": [{"id": "m-test", "object": "model", "owned_by": "mock"}]
                    })),
                )
            } else {
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({
                        "error": {"message": "Incorrect API key", "type": "invalid_request_error"}
                    })),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    MockUpstream {
        base_url: format!("http://{addr}"),
    }
}

/// Builds the control router over a test DB with test defaults.
///
/// The login throttle limit is raised far above production (5) so shared-DB
/// Postgres tests running in parallel can probe bad credentials without
/// tripping A-004's bucket; the dedicated throttle test builds its own app.
pub async fn control_app(db: Db) -> (Router, MockUpstream) {
    let mock = spawn_models_mock().await;
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    let mut state = tributary_control::ControlState::new(
        db,
        http,
        tributary_crypto::MasterKey::generate(),
        vec!["m-test".to_string()],
    );
    state.login_max_fails = 10_000;
    (tributary_control::router(state), mock)
}

/// Builds a control router with a chosen throttle limit (A-004 test).
pub async fn control_app_with_limit(db: Db, max_fails: i64) -> Router {
    let mut state = tributary_control::ControlState::new(
        db,
        reqwest::Client::new(),
        tributary_crypto::MasterKey::generate(),
        vec!["m-test".to_string()],
    );
    state.login_max_fails = max_fails;
    tributary_control::router(state)
}

/// Bootstrap + login → session token.
///
/// The Postgres half of the dual-DB suite shares one database between
/// concurrently-running tests: the first test creates the shared admin,
/// every other test loses the race (refusal or unique violation) and
/// simply logs in with the same credentials. SQLite keeps one DB per
/// test, so the create always succeeds there.
///
/// The login retries briefly: a losing creator may reach its login while
/// the winning INSERT is still uncommitted (the user is momentarily
/// invisible → 401). Retrying turns that race into a short wait instead
/// of a flaky unwrap.
pub async fn bootstrap_and_login(db: &Db, app: &Router) -> String {
    let _ = tributary_control::create_host_admin(db, "root@tributary.dev", "hunter2hunter2").await;
    let creds = serde_json::json!({"email": "root@tributary.dev", "password": "hunter2hunter2"});
    for _ in 0..40 {
        let (_, body) = send(app, post_json("/api/v1/auth/login", None, &creds)).await;
        if let Some(token) = body["token"].as_str() {
            return token.to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("shared admin login never succeeded (bootstrap race or credentials)");
}

/// Fires a request at the router, returning status + parsed JSON body.
pub async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let (status, _headers, value) = send_full(router, req).await;
    (status, value)
}

/// Fires a request at the router, returning status + headers + JSON body —
/// needed where the contract lives in headers (Set-Cookie, Retry-After).
pub async fn send_full(
    router: &Router,
    req: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let response = router.clone().oneshot(req).await.expect("oneshot");
    let status = response.status();
    let headers = response.headers().clone();
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
    (status, headers, value)
}

/// POST with a JSON body (and optional bearer token).
pub fn post_json(uri: &str, token: Option<&str>, body: &Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// POST JSON plus extra headers (e.g. `X-Forwarded-For` for throttle tests).
pub fn post_json_with_headers(
    uri: &str,
    token: Option<&str>,
    body: &Value,
    extra: &[(&str, &str)],
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// GET with an optional bearer token.
pub fn get_json(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

/// GET carrying a raw Cookie header (browser-session simulation).
pub fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

/// POST empty body carrying a raw Cookie header (logout simulation).
pub fn post_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

/// DELETE with an optional bearer token.
pub fn delete_json(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("DELETE").uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

/// Seeds one metered usage row directly — in production only the gateway's
/// chat path writes these; tests shortcut it (the control plane only reads).
#[allow(clippy::too_many_arguments)]
pub async fn insert_usage_event(
    db: &Db,
    id: &str,
    ts: i64,
    pool_key_id: &str,
    user_id: Option<&str>,
    pool_id: Option<&str>,
    model: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
) {
    let sql = db.sql(
        "INSERT INTO usage_events \
         (id, ts, pool_key_id, user_id, pool_id, provider_key_id, model, \
          prompt_tokens, completion_tokens, cost_micros, latency_ms, status) \
         VALUES (?, ?, ?, ?, ?, NULL, ?, ?, ?, 0, 42, '200')",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .bind(ts)
        .bind(pool_key_id)
        .bind(user_id)
        .bind(pool_id)
        .bind(model)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .execute(db.pool())
        .await
        .expect("usage seed");
}

/// Unique-per-run suffix so parallel tests never collide on unique columns.
pub fn unique_suffix() -> String {
    let mut bytes = [0u8; 4];
    use rand::RngCore;
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Unique slug from a prefix (lowercase, dashes — matches validation).
pub fn unique_slug(prefix: &str) -> String {
    format!("{prefix}-{}", unique_suffix())
}

/// Unique email from a prefix.
pub fn unique_email(prefix: &str) -> String {
    format!("{prefix}-{}@test.tributary.dev", unique_suffix())
}
