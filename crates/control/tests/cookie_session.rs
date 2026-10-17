//! Cookie-session tests (M3, SPEC §7): login sets a secure HttpOnly
//! SameSite=Lax cookie; the cookie authenticates like the bearer token;
//! logout expires it.

mod harness;

use axum::http::StatusCode;
use serde_json::json;

use harness::*;

/// Extracts the `tributary_session` cookie pair from a Set-Cookie header.
fn session_cookie_value(headers: &axum::http::HeaderMap) -> String {
    headers
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .find_map(|v| {
            let raw = v.to_str().ok()?;
            raw.starts_with("tributary_session=").then(|| {
                // Header form `tributary_session=<val>; Path=…` → the pair
                // the browser would echo back verbatim.
                raw.split(';').next().unwrap().to_string()
            })
        })
        .expect("session cookie in Set-Cookie")
}

#[tokio::test]
async fn login_sets_secure_cookie_and_cookie_authenticates() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        // Shared-DB race tolerance (PG half): another parallel test may have
        // created the admin first — the login below proves it exists.
        let _ =
            tributary_control::create_host_admin(&db.pool, "root@tributary.dev", "hunter2hunter2")
                .await;

        let (status, headers, body) = send_full(
            &app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": "root@tributary.dev", "password": "hunter2hunter2"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);
        assert!(body["token"].as_str().unwrap().starts_with("ses-"));
        assert_eq!(body["user"]["role"], "host_admin");
        assert_eq!(body["user"]["email"], "root@tributary.dev");

        // Cookie contract (SPEC §7 + director directives 2/3).
        let cookie = session_cookie_value(&headers);
        for attr in ["HttpOnly", "Secure", "SameSite=Lax"] {
            let set_cookie = headers
                .get_all(axum::http::header::SET_COOKIE)
                .iter()
                .map(|v| v.to_str().unwrap_or_default())
                .collect::<String>();
            assert!(
                set_cookie.contains(attr),
                "{}: Set-Cookie missing {attr}: {set_cookie}",
                db.name
            );
        }
        let _ = cookie;

        // The cookie authenticates /auth/me exactly like the bearer would.
        let cookie_pair = session_cookie_value(&headers);
        let (status, me) = send(&app, get_with_cookie("/api/v1/auth/me", &cookie_pair)).await;
        assert_eq!(status, StatusCode::OK, "{}: {me}", db.name);
        assert_eq!(me["email"], "root@tributary.dev");
        assert_eq!(me["role"], "host_admin");

        // ...and it authorizes real API reads (cookie-scoped session).
        let (status, _) = send(&app, get_with_cookie("/api/v1/orgs", &cookie_pair)).await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);

        // Logout (cookie-authenticated) expires the cookie + kills the row.
        let (status, headers2, _) =
            send_full(&app, post_with_cookie("/api/v1/auth/logout", &cookie_pair)).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{}", db.name);
        let expired = headers2
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap_or_default())
            .collect::<String>();
        assert!(expired.contains("Max-Age=0"), "{}: {expired}", db.name);

        // The session is really gone (cookie AND bearer both dead).
        let (status, _) = send(&app, get_with_cookie("/api/v1/auth/me", &cookie_pair)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", db.name);
        let bearer = body["token"].as_str().unwrap();
        let (status, _) = send(&app, get_json("/api/v1/orgs", Some(bearer))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", db.name);
    }
}

/// Cookie sessions work for members too (self-service login path).
#[tokio::test]
async fn member_can_login_with_cookie_for_self_service() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        let host = bootstrap_and_login(&db.pool, &app).await;
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&host),
                &json!({"name": "CookieCo", "slug": unique_slug("cookie")}),
            ),
        )
        .await;
        let email = unique_email("mem");
        let (_, member) = send(
            &app,
            post_json(
                "/api/v1/members",
                Some(&host),
                &json!({"org_id": org["id"].as_str().unwrap(), "email": email, "password": "password123"}),
            ),
        )
        .await;
        let _ = member;

        let (_, headers, body) = send_full(
            &app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": email, "password": "password123"}),
            ),
        )
        .await;
        assert_eq!(body["user"]["role"], "member", "{}", db.name);
        let cookie = session_cookie_value(&headers);

        // Self-service reads fine…
        let (status, keys) = send(&app, get_with_cookie("/api/v1/me/keys", &cookie)).await;
        assert_eq!(status, StatusCode::OK, "{}: {keys}", db.name);
        // …admin surface stays 403 for members (uniform, pre-handler).
        let (status, _) = send(&app, get_with_cookie("/api/v1/orgs", &cookie)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{}", db.name);
    }
}
