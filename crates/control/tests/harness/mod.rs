//! Shared test harness: dual-DB pools, mock upstream, control router, and
//! HTTP helpers. Never contacts a real provider.

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::routing::get;
use serde_json::Value;
use tributary_common::db::{Db, run_migrations};
use tributary_control::{ControlState, router};

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
            Err(err) => eprintln!("postgres tests skipped (connect failed): {err:#}"),
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
pub async fn control_app(db: Db) -> (Router, MockUpstream) {
    let mock = spawn_models_mock().await;
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    let state = ControlState {
        db,
        http,
        master: tributary_crypto::MasterKey::generate(),
        default_models: vec!["m-test".to_string()],
    };
    (router(state), mock)
}

/// Bootstrap + login → session token.
///
/// The Postgres half of the dual-DB suite shares one database between
/// concurrently-running tests: the first test creates the shared admin,
/// every other test loses the race (refusal or unique violation) and
/// simply logs in with the same credentials. SQLite keeps one DB per
/// test, so the create always succeeds there.
pub async fn bootstrap_and_login(db: &Db, app: &Router) -> String {
    let _ = tributary_control::create_host_admin(db, "root@tributary.dev", "hunter2hunter2").await;
    let (_, body) = send(
        app,
        post_json(
            "/api/v1/auth/login",
            None,
            &serde_json::json!({"email": "root@tributary.dev", "password": "hunter2hunter2"}),
        ),
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}

/// Fires a request at the router, returning status + parsed JSON body.
pub async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Value) {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let response = router.clone().oneshot(req).await.expect("oneshot");
    let status = response.status();
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
    (status, value)
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

/// GET with an optional bearer token.
pub fn get_json(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
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
