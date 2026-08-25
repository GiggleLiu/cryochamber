//! The WebView's Content-Security-Policy is a security property that nothing
//! else in this crate can see: it lives as a string in `tauri.conf.json`, is
//! consumed by Tauri at runtime, and a directive deleted by a well-meaning edit
//! compiles, links, boots, and renders exactly like the policy that was there
//! before. So it is asserted here, against the file the bundle actually ships,
//! in the pattern `capability_http_scope.rs` already established.
//!
//! What each pinned directive is holding up:
//!
//! * `script-src 'self'` — **must be written explicitly**, never left to
//!   `default-src`. On Android, wry appends the sha256 hashes of its injected
//!   initialization scripts to `script-src`
//!   (`wry/src/inject_initialization_scripts.rs`), and when that directive is
//!   *absent* it creates it as `script-src <hashes>` with no `'self'` — which
//!   then blocks our own `/assets/index-*.js` and yields a blank window on the
//!   one platform hardest to debug.
//!
//! * `style-src 'unsafe-inline'` — load-bearing for **math rendering**, and the
//!   reason is markup-parsed `style="…"` attributes, not injected `<style>`
//!   elements. CSP governs style attributes: CSP3 splits them into
//!   `style-src-attr`, which falls back to `style-src` when unspecified, so
//!   `style-src 'self'` alone strips them. `katex.renderToString` puts all of
//!   its layout in those attributes — a single `\frac{a}{b}` emits ten of them —
//!   and `components/sanitize.ts` deliberately keeps `style` (filtered to a
//!   layout allowlist) for exactly that reason. The markup reaches the DOM as an
//!   HTML string through markdown rendering, so it is parsed markup and the
//!   policy applies. React DOM's runtime `<style>` elements need it too, but
//!   they are the secondary reason.
//!
//!   Splitting this into `style-src 'self'; style-src-attr 'unsafe-inline'` was
//!   considered and rejected: a WebView that does not implement
//!   `style-src-attr` ignores it and falls back to `style-src 'self'`, which
//!   silently destroys every formula. The shell runs on whatever WebView the OS
//!   provides, so the tighter spelling is the one that breaks quietly.
//!
//! * `object-src`/`base-uri`/`form-action`/`frame-ancestors 'none'` — the
//!   injection barriers. None of them affects rendering, so nothing would ever
//!   notice their absence except an attacker.
//!
//! * no `unsafe-eval` in production — `devCsp` carries it for Vite; the two
//!   strings sit next to each other in the config, and a copy-paste between
//!   them is exactly the accident worth catching.
//!
//! * `connect-src` with the IPC schemes — Tauri's IPC is
//!   `fetch(ipc://localhost/<cmd>)`, or `http://ipc.localhost/<cmd>` on
//!   Windows/Android. Desktop degrades to a postMessage fallback if it is
//!   blocked; Android's channel-data path does not, and that is what
//!   `pinned_sse` streams over.

use serde_json::Value;

/// The config the shipped bundle is built from, read at compile time.
const CONFIG: &str = include_str!("../tauri.conf.json");

fn security_field(name: &str) -> String {
    let config: Value = serde_json::from_str(CONFIG).expect("valid JSON");
    config["app"]["security"][name]
        .as_str()
        .unwrap_or_else(|| panic!("`app.security.{name}` must be a policy string, not null"))
        .to_string()
}

/// The sources of one directive, as the policy string writes them.
fn directive(policy: &str, name: &str) -> String {
    policy
        .split(';')
        .map(str::trim)
        .find(|d| d.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("the policy is missing a `{name}` directive"))
        .to_string()
}

#[test]
fn the_shell_ships_a_policy_whose_script_src_admits_the_bundle() {
    // Explicit `script-src 'self'`: see the note on wry's Android hash-append.
    let csp = security_field("csp");
    assert!(directive(&csp, "script-src").contains("'self'"));
}

#[test]
fn math_rendering_keeps_its_inline_style_attributes() {
    // `'unsafe-inline'` under `style-src` itself — not `style-src-attr`, which
    // a WebView without CSP3 support would ignore its way past.
    let csp = security_field("csp");
    let style_src = directive(&csp, "style-src");
    assert!(style_src.contains("'unsafe-inline'"));
    assert!(!csp.contains("style-src-attr"));
}

#[test]
fn the_injection_barriers_are_all_present() {
    let csp = security_field("csp");
    for name in ["object-src", "base-uri", "form-action", "frame-ancestors"] {
        assert_eq!(directive(&csp, name), format!("{name} 'none'"));
    }
}

#[test]
fn production_never_borrows_the_dev_policys_unsafe_eval() {
    assert!(!security_field("csp").contains("unsafe-eval"));
}

#[test]
fn connect_src_carries_the_ipc_schemes_and_no_hub() {
    // Every hub request rides the plugin/`pinned_fetch` native side, so a hub
    // origin appearing here would mean page `fetch` had crept back in.
    let connect_src = directive(&security_field("csp"), "connect-src");
    assert!(connect_src.contains(" ipc:"));
    assert!(connect_src.contains("http://ipc.localhost"));
}
