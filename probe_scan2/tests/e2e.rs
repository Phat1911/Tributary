//! Round-2 adversarial probe — end to end through the REAL merged app
//! (gateway data plane + control plane), in-memory SQLite per test.

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use tributary_common::db::{Db, connect_sqlite_memory};
use tributary_crypto::MasterKey;
use tributary_gateway::build_full_app;
use tributary_gateway::config::Config;

// ---------------------------------------------------------------------
// mocks
// ---------------------------------------------------------------------

/// Scriptable classifier endpoint: fixed status+body for every call,
/// counting hits. Serves BOTH dialect paths.
#[derive(Clone)]
struct MockClassifier {
    url: String,
    plan: Arc<Mutex<Plan>>,
}
#[derive(Clone)]
struct Plan {
    status: u16,
    body: String,
    hits: usize,
}
impl Plan {
    fn ok(body: &str) -> Self {
        Plan { status: 200, body: body.to_string(), hits: 0 }
    }
}

async fn spawn_classifier() -> MockClassifier {
    let plan = Arc::new(Mutex::new(Plan::ok(r#"{"alignment": 0.95}"#)));
    let make = |plan: Arc<Mutex<Plan>>| {
        move |_: String| {
            let plan = plan.clone();
            async move {
                let p = {
                    let mut g = plan.lock().unwrap();
                    g.hits += 1;
                    g.clone()
                };
                (StatusCode::from_u16(p.status).unwrap(), HeaderMap::new(), p.body).into_response()
            }
        }
    };
    let app = Router::new()
        .route("/v1/align", post(make(plan.clone())))
        .route("/v1/systemone", post(make(plan.clone())));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = axum::serve(listener, app).await; });
    MockClassifier { url: format!("http://{addr}"), plan }
}

/// OpenAI-shaped upstream mock (chat + models).
async fn spawn_upstream() -> String {
    let app = Router::new()
        .route(
            "/chat/completions",
            post(|| async {
                Json(json!({
                    "id": "cmpl-mock",
                    "object": "chat.completion",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
                }))
            }),
        )
        .route(
            "/models",
            get(|| async {
                Json(json!({"object": "list", "data": [{"id": "m-test", "object": "model"}]}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = axum::serve(listener, app).await; });
    format!("http://{addr}")
}

// ---------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------

struct Stack {
    app: Router,
    db: Db,
    org: String,
    token_a: String,
    token_b: String,
    token_m: String,
    member_id: String,
    pool_id: String,
    key_raw: String,
    key_id: String,
    cls: MockClassifier,
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn login(app: &Router, email: &str) -> String {
    let (s, v) = call(
        app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(&json!({"email": email, "password": "password123"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "login {email}: {v}");
    v["token"].as_str().unwrap().to_string()
}

async fn stack(dual: bool) -> Stack {
    let db = connect_sqlite_memory().await.unwrap();
    let config = Config {
        database_url: "memory".into(),
        master_key: MasterKey::generate(),
        bind: "127.0.0.1:0".into(),
        default_models: vec!["m-test".into()],
        log_json: false,
        smtp: None,
        geoip_db_path: None,
    };
    let app = build_full_app(db.clone(), config).await.unwrap();
    tributary_control::create_host_admin(&db, "root@t.dev", "password123")
        .await
        .unwrap();
    let host = login(&app, "root@t.dev").await;
    let unique = tributary_common::db::new_id();
    let (_, org) = call(
        &app,
        "POST",
        "/api/v1/orgs",
        Some(&host),
        Some(&json!({"name": "Scan2", "slug": format!("scan2-{unique}")})),
    )
    .await;
    let org_id = org["id"].as_str().unwrap().to_string();
    let mk_admin = |email: String| {
        json!({"org_id": org_id, "email": email, "password": "password123", "role": "org_admin"})
    };
    let email_a = format!("a-{unique}@t.dev");
    let email_b = format!("b-{unique}@t.dev");
    let email_m = format!("m-{unique}@t.dev");
    let _ = call(&app, "POST", "/api/v1/members", Some(&host), Some(&mk_admin(email_a.clone()))).await;
    if dual {
        let _ = call(&app, "POST", "/api/v1/members", Some(&host), Some(&mk_admin(email_b.clone()))).await;
    }
    let (_, m) = call(
        &app,
        "POST",
        "/api/v1/members",
        Some(&host),
        Some(&json!({"org_id": org_id, "email": email_m.clone(), "password": "password123", "role": "member"})),
    )
    .await;
    let member_id = m["id"].as_str().unwrap().to_string();
    let token_a = login(&app, &email_a).await;
    let token_b = if dual { login(&app, &email_b).await } else { token_a.clone() };
    let token_m = login(&app, &email_m).await;
    let upstream = spawn_upstream().await;
    let (_, pool) = call(
        &app,
        "POST",
        "/api/v1/pools",
        Some(&token_a),
        Some(&json!({"org_id": org_id, "name": "scan-pool", "model_allowlist": ["m-test"]})),
    )
    .await;
    let pool_id = pool["id"].as_str().unwrap().to_string();
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/v1/pools/{pool_id}/provider-keys"),
        Some(&token_a),
        Some(&json!({"key": "sk-upstream-anything-0123456789", "base_url": upstream})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "provider key: {v}");
    let cls = spawn_classifier().await;
    let (_, key) = call(
        &app,
        "POST",
        &format!("/api/v1/pools/{pool_id}/keys"),
        Some(&token_a),
        Some(&json!({"user_id": member_id})),
    )
    .await;
    Stack {
        app,
        db,
        org: org_id,
        token_a,
        token_b,
        token_m,
        member_id,
        pool_id,
        key_raw: key["raw"].as_str().unwrap().to_string(),
        key_id: key["id"].as_str().unwrap().to_string(),
        cls,
    }
}

impl Stack {
    fn cfg_body(&self, over: Value) -> Value {
        let mut v = json!({
            "base_url": self.cls.url,
            "dialect": "custom",
            "model": "jev-align-1",
            "key": "sk-classifier-probe-0123456789",
            "purpose_statement": "coding only",
            "mode": "auto_block",
            "acknowledge_auto_block": true,
            "alert_threshold": 0.5,
            "auto_block_threshold": 0.15,
            "sampling_per_hour": 100000,
        });
        merge(&mut v, over);
        v
    }

    async fn get_cfg(&self) -> (StatusCode, Value) {
        call(&self.app, "GET", &format!("/api/v1/orgs/{}/classifier", self.org), Some(&self.token_a), None).await
    }

    async fn disclosure(&self) -> Value {
        let (s, v) = call(&self.app, "GET", "/api/v1/me/disclosure", Some(&self.token_m), None).await;
        assert_eq!(s, StatusCode::OK);
        v
    }

    async fn put_cfg(&self, body: Value) -> (StatusCode, Value) {
        call(&self.app, "PUT", &format!("/api/v1/orgs/{}/classifier", self.org), Some(&self.token_a), Some(&body)).await
    }

    async fn approve(&self, id: &str) -> (StatusCode, Value) {
        call(&self.app, "POST", &format!("/api/v1/orgs/{}/pending-actions/{id}/approve", self.org), Some(&self.token_b), None).await
    }

    async fn go_live_req(&self) -> (StatusCode, Value) {
        call(&self.app, "POST", &format!("/api/v1/orgs/{}/classifier/go-live", self.org), Some(&self.token_a), Some(&json!({}))).await
    }

    async fn chat(&self, content: &str) -> StatusCode {
        let (s, _) = call(
            &self.app,
            "POST",
            "/v1/chat/completions",
            Some(&self.key_raw),
            Some(&json!({"model": "m-test", "messages": [{"role": "user", "content": content}]})),
        )
        .await;
        s
    }

    async fn key_status(&self) -> String {
        let (_, keys) = call(&self.app, "GET", &format!("/api/v1/pools/{}/keys/manage", self.pool_id), Some(&self.token_a), None).await;
        keys.as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == json!(self.key_id))
            .map(|k| k["status"].as_str().unwrap_or("?").to_string())
            .unwrap_or_else(|| "missing".into())
    }

    async fn open_flags(&self, kind: &str) -> Vec<Value> {
        let (_, flags) = call(&self.app, "GET", "/api/v1/review-flags?status=open", Some(&self.token_a), None).await;
        flags
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["kind"] == json!(kind) && f["pool_key_id"] == json!(self.key_id))
            .cloned()
            .collect()
    }

    async fn unblock_if_suspended(&self) {
        if self.key_status().await == "suspended" {
            let flags = self.open_flags("classifier_auto_block").await;
            let id = flags[0]["id"].as_str().unwrap().to_string();
            let (st, _) = call(
                &self.app,
                "POST",
                &format!("/api/v1/review-flags/{id}/resolve"),
                Some(&self.token_a),
                Some(&json!({"action": "unblock"})),
            )
            .await;
            assert_eq!(st, StatusCode::NO_CONTENT);
        }
    }
}

fn merge(base: &mut Value, over: Value) {
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
}

async fn wait_status(s: &Stack, want: &str) {
    for _ in 0..200 {
        if s.key_status().await == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("key never reached status {want}");
}

async fn wait_suspended_state(s: &Stack, want_suspended: bool) {
    for _ in 0..200 {
        if (s.key_status().await == "suspended") == want_suspended {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("key never reached suspended={want_suspended}");
}

async fn wait_hits(s: &Stack, min: usize) {
    for _ in 0..200 {
        if s.cls.plan.lock().unwrap().hits >= min {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("classifier mock never reached {min} hits");
}

/// Drive an org to LIVE auto-block (dual: via approvals; solo: direct).
async fn reach_live(s: &Stack) -> i64 {
    let (st, v) = s.put_cfg(s.cfg_body(json!({}))).await;
    assert!(st == StatusCode::CREATED || st == StatusCode::ACCEPTED, "configure {st} {v}");
    if st == StatusCode::ACCEPTED {
        let (st, v) = s.approve(v["id"].as_str().unwrap()).await;
        assert_eq!(st, StatusCode::OK, "approve configure: {v}");
    }
    let (st, v) = s.go_live_req().await;
    assert!(st == StatusCode::OK || st == StatusCode::ACCEPTED, "go-live {st} {v}");
    if st == StatusCode::ACCEPTED {
        let (st, v) = s.approve(v["id"].as_str().unwrap()).await;
        assert_eq!(st, StatusCode::OK, "approve go-live: {v}");
    }
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(cfg["auto_block_live"], json!(true), "must be live");
    cfg["shadow_started_at"].as_i64().unwrap_or(0)
}

// ---------------------------------------------------------------------
// probes
// ---------------------------------------------------------------------

/// Type/case tricks on PUT must fail closed with nothing stored.
#[tokio::test]
async fn e1_put_type_tricks() {
    let s = stack(true).await;
    // String threshold → serde rejection (422).
    let raw = format!(
        r#"{{"base_url":"{}","dialect":"custom","model":"m","key":"sk-classifier-probe-0123456789","purpose_statement":"p","alert_threshold":"0.5"}}"#,
        s.cls.url
    );
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/orgs/{}/classifier", s.org))
        .header(header::AUTHORIZATION, format!("Bearer {}", s.token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(raw))
        .unwrap();
    let resp = s.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "string threshold must 422");
    // Case tricks on mode/dialect → handler 400. For the missing-ack case,
    // explicitly null the ack (the base body carries acknowledge=true).
    for over in [
        json!({"mode": "AUTO_BLOCK", "acknowledge_auto_block": true}),
        json!({"mode": "Auto_Block"}),
        json!({"dialect": "Jev"}),
        json!({"dialect": "custom "}),
        json!({"acknowledge_auto_block": null}),
    ] {
        let (st, v) = s.put_cfg(s.cfg_body(over.clone())).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "expected 400 for {over}: {v}");
    }
    // Nothing stored after all failures.
    let (st, _) = s.get_cfg().await;
    assert_eq!(st, StatusCode::NOT_FOUND, "no config may exist after rejected PUTs");
}

/// Live-preservation semantics incl. the vendor/purpose swap question and
/// disclosure correctness across every transition.
#[tokio::test]
async fn e2_live_preservation_and_vendor_swap() {
    let s = stack(true).await;
    let t1 = reach_live(&s).await;
    // Disclosure matches live state.
    let d = s.disclosure().await;
    assert_eq!(d["classification_active"], json!(true));
    assert_eq!(d["mode"], json!("auto_block"));
    assert_eq!(d["auto_block_live"], json!(true));

    // --- Vendor/purpose/model/key swap, SAME mode + thresholds → live preserved.
    let evil = spawn_classifier().await;
    {
        let mut p = evil.plan.lock().unwrap();
        p.body = r#"{"alignment": 0.05}"#.into(); // suspends everything
    }
    let body = s.cfg_body(json!({
        "base_url": evil.url,
        "purpose_statement": "a completely different purpose definition",
        "model": "other-vendor-model-x",
        "key": "sk-other-vendor-key-9876543210",
    }));
    let (st, v) = s.put_cfg(body).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let (st, v) = s.approve(v["id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(cfg["auto_block_live"], json!(true), "vendor swap preserves live (as designed)");
    assert_eq!(cfg["shadow_started_at"].as_i64(), Some(t1), "shadow window NOT restarted");
    assert_eq!(cfg["purpose_statement"], json!("a completely different purpose definition"));

    // Member sends perfectly innocent text → new vendor scores 0.05 → key
    // suspended with ZERO shadow data under the new enforcement semantics.
    assert_eq!(s.chat("innocuous homework help please").await, StatusCode::OK);
    wait_status(&s, "suspended").await;
    assert!(!s.open_flags("classifier_auto_block").await.is_empty());
    // Disclosure still truthful.
    let d = s.disclosure().await;
    assert_eq!(d["auto_block_live"], json!(true));

    // Unblock (one-click) restores the key.
    {
        let mut p = evil.plan.lock().unwrap();
        p.body = r#"{"alignment": 0.95}"#.into();
    }
    s.unblock_if_suspended().await;
    assert_eq!(s.key_status().await, "active");
    assert_eq!(s.chat("hello again").await, StatusCode::OK);

    // --- Threshold change (0.15 → 0.2) → re-shadow.
    let (st, v) = s.put_cfg(s.cfg_body(json!({"base_url": evil.url, "auto_block_threshold": 0.2}))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let (st, v) = s.approve(v["id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(cfg["auto_block_live"], json!(false), "threshold change must re-shadow");
    assert!(cfg["shadow_started_at"].as_i64().unwrap_or(0) > t1, "fresh shadow window");
    let d = s.disclosure().await;
    assert_eq!(d["auto_block_live"], json!(false), "banner tracks re-shadow");
}

/// Stale approved payload must NOT restore live once a threshold change
/// re-shadowed (approve-ordering bypass attempt).
#[tokio::test]
async fn e3_no_live_bypass_via_stale_payload() {
    let s = stack(true).await;
    let _t1 = reach_live(&s).await;
    // Change threshold → approve → shadow.
    let (st, v) = s.put_cfg(s.cfg_body(json!({"auto_block_threshold": 0.2}))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let (st, v) = s.approve(v["id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(cfg["auto_block_live"], json!(false));
    // Now PUT back the ORIGINAL thresholds (identical to the live era).
    let (st, v) = s.put_cfg(s.cfg_body(json!({"auto_block_threshold": 0.15}))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let (st, v) = s.approve(v["id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(
        cfg["auto_block_live"],
        json!(false),
        "re-approving the original thresholds must NOT restore live — prior is not live"
    );
    // Only an explicit go-live can re-arm.
    let (st, v) = s.go_live_req().await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let (st, v) = s.approve(v["id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(cfg["auto_block_live"], json!(true));
}

/// go_live approved AFTER a threshold-change configure executes against
/// CURRENT state: the NEW thresholds go live with a zero-second shadow.
#[tokio::test]
async fn e4_golive_after_configure_ordering() {
    let s = stack(true).await;
    // Shadow at 0.15.
    let (st, v) = s.put_cfg(s.cfg_body(json!({}))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let (st, _) = s.approve(v["id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK);
    // Request go-live (deferred) but do NOT approve yet.
    let (st, g) = s.go_live_req().await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let g_id = g["id"].as_str().unwrap().to_string();
    // Meanwhile: threshold change to 0.25, approved.
    let (st, v) = s.put_cfg(s.cfg_body(json!({"auto_block_threshold": 0.25}))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let (st, _) = s.approve(v["id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK);
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(cfg["auto_block_live"], json!(false), "re-shadowed at 0.25");
    let t2 = cfg["shadow_started_at"].as_i64().unwrap();
    // NOW approve the go-live → flips the NEW config live instantly.
    let (st, v) = s.approve(&g_id).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, cfg) = s.get_cfg().await;
    assert_eq!(cfg["auto_block_live"], json!(true), "go-live executes against CURRENT config");
    assert_eq!(cfg["auto_block_threshold"], json!(0.25));
    assert_eq!(cfg["shadow_started_at"].as_i64(), Some(t2), "no fresh shadow granted");
    // Enforcement uses the new threshold: 0.95 fine, 0.10 suspends.
    assert_eq!(s.chat("high alignment text").await, StatusCode::OK);
    wait_hits(&s, 1).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(s.key_status().await, "active", "0.95 must not suspend at 0.25");
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.body = r#"{"alignment": 0.10}"#.into();
    }
    assert_eq!(s.chat("low alignment text").await, StatusCode::OK);
    wait_status(&s, "suspended").await;
}

/// Cross-org approve is unreachable; self-approve refused; replay 409.
#[tokio::test]
async fn e5_cross_org_self_replay() {
    let s = stack(true).await; // org X with admins A(token_a)/B(token_b)
    // A second org Y with its own admin C.
    let host = login(&s.app, "root@t.dev").await;
    let unique = tributary_common::db::new_id();
    let (_, org_y) = call(
        &s.app,
        "POST",
        "/api/v1/orgs",
        Some(&host),
        Some(&json!({"name": "Y", "slug": format!("scan2y-{unique}")})),
    )
    .await;
    let org_y = org_y["id"].as_str().unwrap().to_string();
    let email_c = format!("c-{unique}@t.dev");
    let _ = call(
        &s.app,
        "POST",
        "/api/v1/members",
        Some(&host),
        Some(&json!({"org_id": org_y, "email": email_c, "password": "password123", "role": "org_admin"})),
    )
    .await;
    let token_c = login(&s.app, &email_c).await;
    // Org X configure → pending.
    let (st, v) = s.put_cfg(s.cfg_body(json!({"mode": "alert"}))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let pid = v["id"].as_str().unwrap().to_string();
    // C (org Y admin) tries to approve X's action through X's path.
    let (st, _) = call(
        &s.app,
        "POST",
        &format!("/api/v1/orgs/{}/pending-actions/{pid}/approve", s.org),
        Some(&token_c),
        None,
    )
    .await;
    assert!(st == StatusCode::FORBIDDEN || st == StatusCode::NOT_FOUND, "cross-org approve must fail (got {st})");
    // And through Y's own path (id lives in X) → 404.
    let (st, _) = call(
        &s.app,
        "POST",
        &format!("/api/v1/orgs/{org_y}/pending-actions/{pid}/approve"),
        Some(&token_c),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "org-scoped fetch must 404");
    // Self-approve refused.
    let (st, _) = call(
        &s.app,
        "POST",
        &format!("/api/v1/orgs/{}/pending-actions/{pid}/approve", s.org),
        Some(&s.token_a),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // B approves; replay then 409s.
    let (st, _) = s.approve(&pid).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = s.approve(&pid).await;
    assert_eq!(st, StatusCode::CONFLICT, "replay must 409");
}

/// Config deleted between request and approval: approving the configure
/// RESURRECTS the config (sealed key intact), fresh shadow, not live.
#[tokio::test]
async fn e6_delete_then_approve_configure() {
    let s = stack(true).await;
    let (st, v) = s.put_cfg(s.cfg_body(json!({}))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let cfg_id = v["id"].as_str().unwrap().to_string();
    let (st, v) = call(
        &s.app,
        "DELETE",
        &format!("/api/v1/orgs/{}/classifier", s.org),
        Some(&s.token_a),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let del_id = v["id"].as_str().unwrap().to_string();
    let (st, _) = s.approve(&del_id).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = s.get_cfg().await;
    assert_eq!(st, StatusCode::NOT_FOUND, "deleted");
    // Approving the older configure brings the config BACK.
    let (st, v) = s.approve(&cfg_id).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, cfg) = s.get_cfg().await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(cfg["mode"], json!("auto_block"));
    assert_eq!(cfg["auto_block_live"], json!(false), "resurrected config starts in shadow");
    // Sealed key still decrypts: scoring runs end to end.
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.body = r#"{"alignment": 0.05}"#.into();
    }
    assert_eq!(s.chat("some text").await, StatusCode::OK);
    wait_hits(&s, 1).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(s.key_status().await, "active", "shadow must not suspend");
    assert!(!s.open_flags("classifier_flag").await.is_empty(), "alert-mode flag recorded");
}

/// scrub_legacy_approval_audit vs crafted legacy rows: invalid JSON,
/// arrays, huge bodies — no panic, no corruption, idempotent.
#[tokio::test]
async fn e7_scrub_robustness() {
    let db = connect_sqlite_memory().await.unwrap();
    tributary_common::db::run_migrations(&db).await.unwrap();
    let ins = |id: &'static str, action: &'static str, after: String| {
        let db = db.clone();
        async move {
            let sql = db.sql(
                "INSERT INTO audit_log (id, org_id, actor_id, actor_email, action, target_kind, \
                 target_id, before_json, after_json, ip, ts) VALUES (?, NULL, 'u', 'u@t.dev', ?, \
                 'org', 't', NULL, ?, NULL, 1)",
            );
            sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(id)
                .bind(action)
                .bind(&after)
                .execute(db.pool())
                .await
                .unwrap();
        }
    };
    let good = r#"{"mode":"auto_block","ciphertext":"SEALED","dek_wrapped":"WRAP","dialect":"jev"}"#.to_string();
    ins("r1", "approval.requested", good).await;
    ins("r2", "approval.requested", r#"{"broken": "ciphertext""#.to_string()).await; // invalid JSON
    ins("r3", "approval.requested", r#"["dek_wrapped"]"#.to_string()).await; // array
    let huge_inner = format!("{{\"purpose\":\"{}\"}}", "ciphertext ".repeat(20_000));
    ins("r4", "approval.requested", huge_inner).await; // ~260KB, no top-level blob
    ins("r5", "other.action", r#"{"ciphertext":"X"}"#.to_string()).await; // not selected

    let n = tributary_control::audit::scrub_legacy_approval_audit(&db).await;
    assert!(n >= 1, "at least r1 scrubbed (got {n})");
    // r1 cleaned.
    let sql = db.sql("SELECT after_json AS a FROM audit_log WHERE id = 'r1'");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).fetch_one(db.pool()).await.unwrap();
    let after: String = sqlx::Row::get(&row, "a");
    assert!(!after.contains("SEALED") && !after.contains("WRAP"), "r1 scrubbed: {after}");
    assert!(after.contains("auto_block"));
    // r2/r3 untouched (skipped, not corrupted).
    for (id, needle) in [("r2", "ciphertext"), ("r3", "dek_wrapped")] {
        let sql = db.sql(&format!("SELECT after_json AS a FROM audit_log WHERE id = '{id}'"));
        let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).fetch_one(db.pool()).await.unwrap();
        let v: String = sqlx::Row::get(&row, "a");
        assert!(v.contains(needle), "{id} untouched: {v}");
    }
    // Idempotent on r1: second run never mangles the cleaned row.
    let _ = tributary_control::audit::scrub_legacy_approval_audit(&db).await;
    let sql = db.sql("SELECT after_json AS a FROM audit_log WHERE id = 'r1'");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).fetch_one(db.pool()).await.unwrap();
    let after2: String = sqlx::Row::get(&row, "a");
    assert_eq!(after, after2, "scrub must be idempotent");
}

/// Fail-open battery A: 429 → 60s cooldown → member always served and the
/// endpoint NOT hammered (cooldown skip observable).
#[tokio::test]
async fn e8a_fail_open_429_cooldown() {
    let s = stack(false).await; // solo org: direct execution
    let _ = reach_live(&s).await;
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.status = 429;
    }
    assert_eq!(s.chat("trigger 429").await, StatusCode::OK);
    wait_hits(&s, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await; // let the trip land
    // Within the 60s cooldown: scoring skipped (hits frozen), member fine.
    assert_eq!(s.chat("during cooldown").await, StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(s.cls.plan.lock().unwrap().hits, 1, "cooldown must skip the endpoint");
    assert_eq!(s.key_status().await, "active");
}

/// Fail-open battery B: 5xx, NaN, garbage — member never blocked; the
/// underflow-to-zero (1e-400) IS enforced as data (finite → alignment 0).
#[tokio::test]
async fn e8b_fail_open_garbage_and_underflow() {
    let s = stack(false).await;
    let _ = reach_live(&s).await;
    // 500 → member 200.
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.status = 500;
        p.body = "oops".into();
    }
    assert_eq!(s.chat("trigger 500").await, StatusCode::OK);
    wait_hits(&s, 1).await;
    assert_eq!(s.key_status().await, "active");
    // NaN string → fail open (A-014).
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.status = 200;
        p.body = r#"{"alignment": "NaN"}"#.into();
    }
    assert_eq!(s.chat("trigger nan").await, StatusCode::OK);
    wait_hits(&s, 2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(s.key_status().await, "active", "NaN must never suspend");
    // Garbage body → fail open.
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.body = "<html>boom</html>".into();
    }
    assert_eq!(s.chat("trigger garbage").await, StatusCode::OK);
    wait_hits(&s, 3).await;
    assert_eq!(s.key_status().await, "active");
    // 1e-400 as a STRING underflows to Ok(0.0) — FINITE DATA → worst
    // score → suspends under live auto-block (traced: f64 FromStr returns
    // Ok(0.0); only non-finite/unparseable values fail open).
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.body = r#"{"alignment": "1e-400"}"#.into();
    }
    assert_eq!(s.chat("trigger underflow").await, StatusCode::OK);
    wait_hits(&s, 4).await;
    wait_status(&s, "suspended").await;
}

/// Corrupt sealed blob → scoring task fails open, member unaffected.
#[tokio::test]
async fn e9_corrupt_sealed_blob() {
    let s = stack(false).await;
    let _ = reach_live(&s).await;
    {
        let mut p = s.cls.plan.lock().unwrap();
        p.body = r#"{"alignment": 0.01}"#.into();
    }
    let sql = s.db.sql("UPDATE classifier_config SET ciphertext = 'zzz-not-base64'");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .execute(s.db.pool())
        .await
        .unwrap();
    assert_eq!(s.chat("with corrupt blob").await, StatusCode::OK, "member must be served");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(s.key_status().await, "active", "decrypt failure must fail open");
    // Control plane unaffected.
    let (st, _) = s.get_cfg().await;
    assert_eq!(st, StatusCode::OK);
}

/// Strict '<' at the threshold in bp, including float rounding at the
/// boundary (0.5 and 0.49999 both land on 5000 bp → NOT blocked).
#[tokio::test]
async fn e10_strict_less_than() {
    let s = stack(false).await;
    let _ = reach_live(&s).await;
    let (st, _) = s.put_cfg(s.cfg_body(json!({"alert_threshold": 0.6, "auto_block_threshold": 0.5}))).await;
    assert_eq!(st, StatusCode::CREATED, "solo re-PUT with changed thresholds → shadow");
    let (st, _) = s.go_live_req().await;
    assert_eq!(st, StatusCode::OK);
    let mut prev_hits = 0usize;
    for (body, expect_suspended) in [
        (r#"{"alignment": 0.5}"#, false),
        (r#"{"alignment": 0.49999}"#, false), // rounds to 5000 bp
        (r#"{"alignment": 0.4999}"#, true),   // 4999 bp < 5000
    ] {
        s.unblock_if_suspended().await;
        {
            let mut p = s.cls.plan.lock().unwrap();
            p.body = body.to_string();
        }
        assert_eq!(s.chat("boundary probe").await, StatusCode::OK);
        // Wait for THIS round's score to be consumed by the endpoint.
        for _ in 0..200 {
            if s.cls.plan.lock().unwrap().hits > prev_hits {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        prev_hits = s.cls.plan.lock().unwrap().hits;
        // Give the scoring task time to (not) suspend, then settle.
        for _ in 0..20 {
            if (s.key_status().await == "suspended") == expect_suspended {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            s.key_status().await == "suspended",
            expect_suspended,
            "score {body} must suspend={expect_suspended}"
        );
    }
}
