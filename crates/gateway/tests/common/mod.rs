//! Shared integration-test harness: dual-DB pools (SQLite tempfile + Postgres
//! via env) and a hand-rolled axum mock upstream. Never contacts real OpenAI.

#![allow(dead_code)] // helpers are shared across test binaries' needs

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
#[cfg(not(windows))]
use sqlx::sqlite::SqliteConnectOptions;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tributary_gateway::config::Config;
use tributary_gateway::db;
use tributary_gateway::keys;
use tributary_gateway::{build_app, state::AppState};

/// A database under test plus anything that must outlive the pool.
pub struct TestDb {
    pub name: &'static str,
    pub pool: db::Db,
    _keep_alive: Option<tempfile::TempDir>,
}

/// SQLite (always, tempfile per AGENTS test convention) + Postgres when
/// `TRIBUTARY_TEST_DATABASE_URL` is set (CI always sets it).
pub async fn test_dbs() -> Vec<TestDb> {
    let mut dbs = vec![sqlite_db().await];
    match std::env::var("TRIBUTARY_TEST_DATABASE_URL") {
        Ok(url) => match db::Db::connect(&url).await {
            Ok(db) => {
                db::run_migrations(&db).await.expect("postgres migrations");
                dbs.push(TestDb {
                    name: "postgres",
                    pool: db,
                    _keep_alive: None,
                });
            }
            Err(err) => eprintln!("postgres tests skipped (connect failed): {err:#}"),
        },
        Err(_) => eprintln!(
            "TRIBUTARY_TEST_DATABASE_URL unset — Postgres half of the dual-DB \
             suite is skipped (CI always sets it)"
        ),
    }
    dbs
}

async fn sqlite_db() -> TestDb {
    // Linux/CI: real tempfile — absolute /tmp paths round-trip through the
    // sqlx URL form fine.
    #[cfg(not(windows))]
    {
        use sqlx::ConnectOptions;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("gateway-test.db");
        // Pre-create the file (zero length = valid empty SQLite db) because
        // the URL round-trip does not carry create_if_missing.
        std::fs::File::create(&file).expect("pre-create sqlite file");
        let url = SqliteConnectOptions::new()
            .filename(&file)
            .to_url_lossy()
            .to_string();
        let pool = db::Db::connect(&url).await.expect("sqlite connect");
        db::run_migrations(&pool).await.expect("sqlite migrations");
        TestDb {
            name: "sqlite",
            pool,
            _keep_alive: Some(dir),
        }
    }
    // Windows-local (cargo.exe interop) only: sqlx 0.9's Any driver takes
    // URLs exclusively and its URL round-trip cannot express absolute
    // Windows paths (the drive colon parses as a port separator), nor do
    // relative paths resolve from the UNC test cwd. Tests therefore use a
    // single-connection in-memory database; CI on Linux always exercises
    // the real tempfile path above.
    #[cfg(windows)]
    {
        let pool = db::connect_sqlite_memory().await.expect("sqlite memory");
        db::run_migrations(&pool).await.expect("sqlite migrations");
        TestDb {
            name: "sqlite",
            pool,
            _keep_alive: None,
        }
    }
}

/// Seeds one pool key; returns `(raw_key, row_id)`.
pub async fn seed_key(
    pool: &db::Db,
    status: &str,
    expires_at: Option<i64>,
    user_id: Option<&str>,
) -> (String, String) {
    let key = keys::generate_key();
    let id = db::new_id();
    db::insert_pool_key(
        pool,
        &db::NewPoolKey {
            id: id.clone(),
            key_hash: key.key_hash,
            key_prefix: key.key_prefix,
            kind: "live".to_string(),
            status: status.to_string(),
            expires_at,
            user_id: user_id.map(str::to_string),
            created_at: db::now_ms(),
        },
    )
    .await
    .expect("seed insert");
    (key.raw, id)
}

/// The upstream mock's scripted behavior.
#[derive(Clone)]
pub enum MockMode {
    /// 200 JSON completion with usage {prompt_tokens: 11, completion_tokens: 7}.
    NonStream,
    /// 200 SSE stream; `with_usage` controls the final usage chunk.
    Stream { with_usage: bool },
    /// JSON error with the given status.
    Error(u16),
}

#[derive(Debug, Clone)]
pub struct Captured {
    pub auth: Option<String>,
    pub body: serde_json::Value,
}

#[derive(Clone)]
pub struct MockUpstream {
    pub base_url: String,
    pub captured: Arc<Mutex<Vec<Captured>>>,
}

pub async fn spawn_mock(mode: MockMode) -> MockUpstream {
    let captured: Arc<Mutex<Vec<Captured>>> = Arc::new(Mutex::new(Vec::new()));
    let state = (mode, captured.clone());

    let app = Router::new()
        .route(
            "/chat/completions",
            post(
                |State(state): State<(MockMode, Arc<Mutex<Vec<Captured>>>)>,
                 headers: HeaderMap,
                 body: Bytes| async move {
                    let (mode, captured) = state;
                    {
                        let mut guard = captured.lock().unwrap();
                        guard.push(Captured {
                            auth: headers
                                .get(header::AUTHORIZATION)
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                            body: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
                        });
                    }
                    match mode {
                        MockMode::NonStream => Json(serde_json::json!({
                            "id": "chatcmpl-mock",
                            "object": "chat.completion",
                            "model": "m-test",
                            "choices": [{
                                "index": 0,
                                "message": {"role": "assistant", "content": "Hello!"},
                                "finish_reason": "stop",
                            }],
                            "usage": {"prompt_tokens": 11, "completion_tokens": 7},
                        }))
                        .into_response(),
                        MockMode::Stream { with_usage } => Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(sse_script(with_usage)))
                            .unwrap(),
                        MockMode::Error(code) => (
                            StatusCode::from_u16(code).unwrap(),
                            Json(serde_json::json!({
                                "error": {"message": "mock boom", "type": "server_error"}
                            })),
                        )
                            .into_response(),
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
    MockUpstream {
        base_url: format!("http://{addr}"),
        captured,
    }
}

fn sse_script(with_usage: bool) -> String {
    let chunk = |delta: &str| {
        format!(
            "data: {{\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m-test\",\"choices\":[{{\"index\":0,\"delta\":{delta}}}]}}\n\n"
        )
    };
    let mut script = String::new();
    script.push_str(&chunk(r#"{"role":"assistant","content":"Hel"}"#));
    script.push_str(&chunk(r#"{"content":"lo"}"#));
    script.push_str("data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n");
    if with_usage {
        script.push_str("data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m-test\",\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3}}\n\n");
    }
    script.push_str("data: [DONE]\n\n");
    script
}

/// Upstream key used by tests — proves pool keys are swapped for provider keys.
pub const TEST_UPSTREAM_KEY: &str = "sk-test-upstream-key";

pub fn test_config(base_url: String) -> Config {
    Config {
        database_url: String::new(),
        openai_api_key: TEST_UPSTREAM_KEY.to_string(),
        openai_base_url: base_url,
        bind: String::new(),
        models: vec!["m-test".to_string(), "m-other".to_string()],
        log_json: false,
    }
}

pub async fn test_app(pool: db::Db, mock: &MockUpstream) -> Router {
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let state = AppState::new(pool, http, test_config(mock.base_url.clone()));
    build_app(state)
}

pub fn chat_request(auth: Option<&str>, body: &serde_json::Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = auth {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// Polls until at least `min` usage events exist for the key (streaming rows
/// are written by a background task after the last byte).
pub async fn wait_for_usage(pool: &db::Db, key_id: &str, min: usize) -> Vec<db::UsageEventRow> {
    for _ in 0..100 {
        let rows = db::usage_events_for_key(pool, key_id)
            .await
            .expect("usage read");
        if rows.len() >= min {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("usage events for key {key_id} never reached {min}");
}

/// A minimal chat request body.
pub fn chat_body() -> serde_json::Value {
    serde_json::json!({
        "model": "m-test",
        "messages": [{"role": "user", "content": "hi"}],
    })
}
