//! Security middleware for the hub web server.
//!
//! Two guards are applied to the whole router in `build_router_with_state`:
//!
//! 1. **Host-header allowlist** — the hub binds loopback by default and can
//!    start/stop chambers and surface provider config. A malicious web page can
//!    point a hostname it controls at `127.0.0.1` (DNS rebinding) and script
//!    requests against the hub. Rejecting any request whose `Host` header host
//!    part is not loopback (or the configured bind host) defeats that: a
//!    cross-origin page cannot forge the victim's `Host` header.
//! 2. **CSRF guard** — every state-changing route is a POST. A cross-origin
//!    *simple* request (form post, `<img>`, top-level navigation) cannot set a
//!    custom request header, so requiring `X-Cryo-CSRF` (or a browser-supplied
//!    same-origin / direct-navigation `Sec-Fetch-Site`) on non-GET/HEAD
//!    requests blocks drive-by state changes while the hub's own `fetch` calls
//!    (which set the header) still work.

use std::sync::Arc;

use axum::{
    extract::Request,
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};

/// Loopback hosts that are always allowed, independent of configuration.
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "::1"];

/// Wrap `router` with the host + CSRF guard. `configured_hosts` are the host
/// names from `HubConfig` — the bind host (so non-default binds keep working)
/// plus `public_hosts` (so a reverse proxy may forward the public hostname
/// rather than rewriting it to loopback).
pub fn apply(router: Router, configured_hosts: Vec<String>) -> Router {
    let allowed = Arc::new(build_allowlist(configured_hosts));
    router.layer(axum::middleware::from_fn(
        move |req: Request, next: Next| {
            let allowed = allowed.clone();
            async move { guard(&allowed, req, next).await }
        },
    ))
}

/// Build the set of allowed host-parts: loopback plus the configured hosts.
fn build_allowlist(configured_hosts: Vec<String>) -> Vec<String> {
    let mut allowed: Vec<String> = LOOPBACK_HOSTS.iter().map(|h| h.to_string()).collect();
    for host in configured_hosts {
        let norm = normalize_host(&host);
        if !norm.is_empty() && !allowed.contains(&norm) {
            allowed.push(norm);
        }
    }
    allowed
}

async fn guard(allowed: &[String], req: Request, next: Next) -> Response {
    // Only browser requests carry a host: the `Host` header (HTTP/1.1) or the
    // `:authority` (HTTP/2). A request with neither comes from a local
    // non-browser client (curl, scripts, the oneshot test harness) and is
    // neither a DNS-rebinding nor a CSRF vector — a browser can never *omit*
    // the host — so it passes through. Any request that names a host is treated
    // as browser-originated and must clear both guards.
    if let Some(host) = request_host(&req) {
        if !host_is_allowed(host, allowed) {
            return StatusCode::FORBIDDEN.into_response();
        }
        if is_state_changing(req.method()) && !csrf_ok(&req) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(req).await
}

/// The host the request targets: the `Host` header (HTTP/1.1) or, failing that,
/// the URI authority (HTTP/2 / absolute-form). `None` means the request carried
/// no host at all.
fn request_host(req: &Request) -> Option<&str> {
    req.headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(host_part)
        .or_else(|| req.uri().host())
}

fn host_is_allowed(host: &str, allowed: &[String]) -> bool {
    let norm = normalize_host(host);
    allowed.iter().any(|a| a == &norm)
}

fn is_state_changing(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD)
}

/// A request passes the CSRF check if it carries the custom `X-Cryo-CSRF`
/// header (which a cross-origin simple request cannot set) or a
/// `Sec-Fetch-Site` the browser marks as same-origin / direct navigation.
fn csrf_ok(req: &Request) -> bool {
    let headers = req.headers();
    if headers.contains_key("x-cryo-csrf") {
        return true;
    }
    match headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        Some(site) => matches!(site, "same-origin" | "none"),
        None => false,
    }
}

/// Strip the `:port` (and any IPv6 brackets) from a `Host` header value.
fn host_part(value: &str) -> &str {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('[') {
        // Bracketed IPv6 literal: `[::1]` or `[::1]:8765`.
        return match rest.find(']') {
            Some(end) => &rest[..end],
            None => rest,
        };
    }
    // An unbracketed IPv6 literal (more than one colon) carries no port suffix.
    if value.matches(':').count() > 1 {
        return value;
    }
    match value.split_once(':') {
        Some((host, _)) => host,
        None => value,
    }
}

/// Lowercase and strip IPv6 brackets so `[::1]` and `::1` compare equal.
fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

#[cfg(test)]
#[path = "../unit_tests/hub/security.rs"]
mod tests;
