use super::*;

use axum::body::Body;
use axum::http::Request;

fn req_with_headers(method: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri("/api/chambers");
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    builder.body(Body::empty()).unwrap()
}

#[test]
fn host_part_strips_port() {
    assert_eq!(host_part("127.0.0.1:8765"), "127.0.0.1");
    assert_eq!(host_part("localhost:3000"), "localhost");
    assert_eq!(host_part("example.com"), "example.com");
}

#[test]
fn host_part_handles_ipv6() {
    assert_eq!(host_part("[::1]:8765"), "::1");
    assert_eq!(host_part("[::1]"), "::1");
    // Unbracketed IPv6 has no port suffix; keep it whole.
    assert_eq!(host_part("::1"), "::1");
}

#[test]
fn normalize_host_lowercases_and_unwraps_brackets() {
    assert_eq!(normalize_host("LOCALHOST"), "localhost");
    assert_eq!(normalize_host("[::1]"), "::1");
    assert_eq!(normalize_host(" 127.0.0.1 "), "127.0.0.1");
}

#[test]
fn build_allowlist_always_contains_loopback() {
    let allowed = build_allowlist(None);
    assert!(allowed.iter().any(|h| h == "127.0.0.1"));
    assert!(allowed.iter().any(|h| h == "localhost"));
    assert!(allowed.iter().any(|h| h == "::1"));
}

#[test]
fn build_allowlist_adds_configured_host_without_duplicating_loopback() {
    let allowed = build_allowlist(Some("cryo.example.com".to_string()));
    assert!(allowed.iter().any(|h| h == "cryo.example.com"));

    // A configured loopback host does not create a duplicate entry.
    let allowed = build_allowlist(Some("127.0.0.1".to_string()));
    assert_eq!(allowed.iter().filter(|h| *h == "127.0.0.1").count(), 1);
}

#[test]
fn request_host_reads_header_then_uri_authority() {
    // Host header present → its host part.
    let req = req_with_headers("GET", &[("host", "127.0.0.1:8765")]);
    assert_eq!(request_host(&req), Some("127.0.0.1"));
    // No host anywhere → None (local non-browser client).
    let req = req_with_headers("GET", &[]);
    assert_eq!(request_host(&req), None);
}

#[test]
fn host_is_allowed_accepts_loopback_and_configured_rejects_others() {
    let allowed = build_allowlist(Some("cryo.example.com".to_string()));

    assert!(host_is_allowed("127.0.0.1", &allowed));
    assert!(host_is_allowed("LOCALHOST", &allowed));
    assert!(host_is_allowed("cryo.example.com", &allowed));
    assert!(!host_is_allowed("evil.example.com", &allowed));
}

#[test]
fn is_state_changing_only_for_unsafe_methods() {
    assert!(!is_state_changing(&Method::GET));
    assert!(!is_state_changing(&Method::HEAD));
    assert!(is_state_changing(&Method::POST));
    assert!(is_state_changing(&Method::DELETE));
    assert!(is_state_changing(&Method::PUT));
}

#[test]
fn csrf_ok_requires_custom_header_or_same_origin_fetch_metadata() {
    // Bare POST with no CSRF signal fails.
    assert!(!csrf_ok(&req_with_headers("POST", &[])));
    // Custom header passes.
    assert!(csrf_ok(&req_with_headers("POST", &[("x-cryo-csrf", "1")])));
    // Same-origin / direct navigation Sec-Fetch-Site passes.
    assert!(csrf_ok(&req_with_headers(
        "POST",
        &[("sec-fetch-site", "same-origin")]
    )));
    assert!(csrf_ok(&req_with_headers(
        "POST",
        &[("sec-fetch-site", "none")]
    )));
    // Cross-site Sec-Fetch-Site fails.
    assert!(!csrf_ok(&req_with_headers(
        "POST",
        &[("sec-fetch-site", "cross-site")]
    )));
}
