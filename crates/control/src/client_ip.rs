//! Client-IP sanitizing middleware (A-019, M5 hardening).
//!
//! ONE enforcement point for the trusted-proxy trust model: before any
//! handler runs, the raw `X-Forwarded-For` (attacker-controlled input) is
//! replaced with the single resolved client IP from
//! [`tributary_common::net::resolve_client_ip`]. Downstream code — the
//! gateway allowlist, D32, travel history, the control plane's audit rows
//! and login-throttle buckets — keeps reading `client_ip(&headers)` and
//! physically cannot observe a forged hop, because the forged header no
//! longer exists by then.
//!
//! The transport socket comes from `into_make_service_with_connect_info`
//! in production; tests inject `ConnectInfo` into the request extensions
//! (the extractor's own read path). No socket info ⇒ fail-closed: the
//! header is dropped, `client_ip` degrades to `"local"`.

use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ConnectInfo;
use axum::extract::State;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use tributary_common::net::Cidr;

/// Resolves the real client IP and rewrites the request's XFF to it.
///
/// `state` is the trusted-proxy list (empty ⇒ trust nothing — socket
/// address decides). Runs per request, on both planes (the gateway layers
/// it over `/v1/*`, the control router over `/api/v1/*`).
pub async fn sanitize_xff(
    State(trusted): State<Arc<Vec<Cidr>>>,
    mut req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let socket: Option<IpAddr> = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());
    let resolved = tributary_common::net::resolve_client_ip(req.headers(), socket, &trusted);
    // Remove EVERY incoming line of the header, then write the resolved
    // address as the single hop. `x-forwarded-for` has no `http::header`
    // constant (it is not an RFC-standard field) — the lowercase name is
    // the canonical HeaderName form.
    req.headers_mut().remove("x-forwarded-for");
    if let Ok(value) = HeaderValue::from_str(&resolved) {
        req.headers_mut().insert("x-forwarded-for", value);
    }
    next.run(req).await
}
