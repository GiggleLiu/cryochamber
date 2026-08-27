//! The capability file decides which hubs the plugin transport may reach, and
//! it decides it in URLPattern, whose rules are not the glob rules the syntax
//! suggests. The rule that matters here: a hostname pattern written *without a
//! port* means the scheme's default port and nothing else. Every hub the app
//! is pointed at is on a port (`127.0.0.1:8765` is the hub's own default), so a
//! portless pattern refuses the entire product.
//!
//! There is no way to ask the plugin: `tauri_plugin_http::scope` is a private
//! module, so neither its `Entry` nor its `Scope` can be named from out here.
//! What this test does instead is match with the plugin's own engine —
//! `urlpattern` 0.3, the version in this crate's lockfile — through a parse
//! that mirrors `scope::parse_url_pattern` (tauri-plugin-http 2.5.9) line for
//! line. The mirror is nine lines and is reproduced verbatim below; if a future
//! plugin changes it, this test keeps asserting the old rules, so the mirror is
//! pinned to a version in its comment and must be re-read when the plugin moves.

use reqwest::Url;
use urlpattern::{UrlPattern, UrlPatternInit, UrlPatternMatchInput};

/// The capability the console window runs under, read at compile time from the
/// file the bundle actually ships.
const CAPABILITY: &str = include_str!("../capabilities/default.json");

/// Verbatim from `tauri-plugin-http` 2.5.9, `src/scope.rs`. Note what it
/// widens — search, hash, an empty or `/` pathname — and what it does not:
/// the port is passed through exactly as written.
fn parse_url_pattern(s: &str) -> UrlPattern {
    let mut init = UrlPatternInit::parse_constructor_string::<regex::Regex>(s, None)
        .unwrap_or_else(|e| panic!("`{s}` is not a valid URL pattern: {e}"));
    if init.search.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
        init.search.replace("*".to_string());
    }
    if init.hash.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
        init.hash.replace("*".to_string());
    }
    if init
        .pathname
        .as_ref()
        .map(|p| p.is_empty() || p == "/")
        .unwrap_or(true)
    {
        init.pathname.replace("*".to_string());
    }
    UrlPattern::parse(init, Default::default()).unwrap_or_else(|e| panic!("`{s}`: {e}"))
}

fn matches(pattern: &UrlPattern, url: &str) -> bool {
    let url = Url::parse(url).expect(url);
    pattern
        .test(UrlPatternMatchInput::Url(url))
        .unwrap_or_default()
}

/// The `allow` list of the `http:default` permission, as the file writes it.
fn http_allow_patterns() -> Vec<String> {
    let capability: serde_json::Value = serde_json::from_str(CAPABILITY).expect("valid JSON");
    let permissions = capability["permissions"]
        .as_array()
        .expect("a permissions array");
    let http = permissions
        .iter()
        .find(|p| p["identifier"] == "http:default")
        .expect("the http:default permission");
    http["allow"]
        .as_array()
        .expect("an allow list")
        .iter()
        .map(|entry| {
            entry["url"]
                .as_str()
                .expect("an allow entry with a url")
                .to_string()
        })
        .collect()
}

/// Is this URL allowed, the way `scope::Scope::is_allowed` asks it — any one
/// entry matching is enough, and there are no denials in this capability.
fn allowed(url: &str) -> bool {
    http_allow_patterns()
        .iter()
        .any(|pattern| matches(&parse_url_pattern(pattern), url))
}

#[test]
fn the_http_scope_reaches_a_hub_on_the_port_it_actually_runs_on() {
    // `cryohub start` binds 127.0.0.1:8765 by default; a hub behind TLS is
    // wherever its operator put it. Both are the ordinary case, and neither is
    // a default port.
    assert!(allowed("http://127.0.0.1:8765/api/whoami"));
    assert!(allowed("https://hub.example:8443/api/events"));
}

#[test]
fn the_http_scope_still_reaches_a_hub_on_a_default_port() {
    // Widening to `:*` must not have traded one half of the range for the
    // other: a hub behind a reverse proxy on 80/443 writes no port at all.
    assert!(allowed("http://hub.local/api/whoami"));
    assert!(allowed("https://hub.example/api/chambers"));
}

#[test]
fn a_pattern_with_no_port_would_allow_only_the_default_one() {
    // Why the entries carry `:*`. This is the shape the capability had, and
    // under URLPattern it refuses every hub the app is ever pointed at.
    let narrow = parse_url_pattern("http://**");
    assert!(!matches(&narrow, "http://127.0.0.1:8765/api/whoami"));
    assert!(matches(&narrow, "http://127.0.0.1/api/whoami"));
}
