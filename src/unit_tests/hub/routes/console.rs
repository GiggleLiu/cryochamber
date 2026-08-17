//! The built-console static route: SPA fallback, real files, containment.

use super::*;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt; // for `oneshot`

use crate::hub::config::HubConfig;
use crate::hub::state::AppState;

/// A minimal stand-in for a vite `dist/`: the SPA entry, one hashed asset, a
/// service worker, and a nested `api/` directory that must never be reachable.
fn fake_dist(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::create_dir_all(root.join("api")).unwrap();
    std::fs::write(root.join("index.html"), "<!doctype html><h1>console</h1>").unwrap();
    std::fs::write(root.join("assets/index-abc123.js"), "export const x = 1;").unwrap();
    std::fs::write(root.join("assets/index-abc123.css"), ":root{color:red}").unwrap();
    std::fs::write(
        root.join("sw.js"),
        "self.addEventListener('install',()=>{})",
    )
    .unwrap();
    std::fs::write(root.join("manifest.webmanifest"), "{\"name\":\"console\"}").unwrap();
    std::fs::write(root.join("api/secret.json"), "{\"leak\":true}").unwrap();
}

/// Router with a fake built console wired in. The tempdirs are returned so the
/// caller keeps them alive for the duration of the request.
fn console_router() -> (tempfile::TempDir, tempfile::TempDir, axum::Router) {
    let workspace = tempfile::tempdir().unwrap();
    let dist = tempfile::tempdir().unwrap();
    fake_dist(dist.path());
    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    let config = HubConfig {
        console_dir: Some(dist.path().to_path_buf()),
        ..HubConfig::default()
    };
    let router = crate::hub::build_router_with_config(app, config);
    (workspace, dist, router)
}

async fn get(router: axum::Router, uri: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let ctype = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, ctype, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
async fn root_serves_the_console_index() {
    let (_ws, _dist, router) = console_router();
    let (status, ctype, body) = get(router, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ctype.starts_with("text/html"), "content-type was {ctype}");
    assert!(body.contains("<h1>console</h1>"), "body was {body}");
}

#[tokio::test]
async fn extensionless_paths_fall_back_to_the_spa_entry() {
    // The console has no client-side router, but any extensionless path a
    // user lands on (a pasted link, a reload) must serve the SPA entry — the
    // app then boots to its home — rather than a 404.
    for uri in ["/c/alpha", "/user_uploads/42/report", "/settings"] {
        let (_ws, _dist, router) = console_router();
        let (status, _, body) = get(router, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} should serve the SPA entry");
        assert!(body.contains("<h1>console</h1>"), "{uri} body was {body}");
    }
}

#[tokio::test]
async fn built_assets_are_served_with_their_content_type() {
    for (uri, expect_type, expect_body) in [
        (
            "/assets/index-abc123.js",
            "text/javascript",
            "export const x = 1;",
        ),
        ("/assets/index-abc123.css", "text/css", ":root{color:red}"),
        (
            "/sw.js",
            "text/javascript",
            "self.addEventListener('install',()=>{})",
        ),
        (
            "/manifest.webmanifest",
            "application/manifest+json",
            "{\"name\":\"console\"}",
        ),
    ] {
        let (_ws, _dist, router) = console_router();
        let (status, ctype, body) = get(router, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} should be served from disk");
        assert!(
            ctype.starts_with(expect_type),
            "{uri} content-type was {ctype}"
        );
        assert_eq!(body, expect_body, "{uri} body");
    }
}

#[tokio::test]
async fn missing_files_do_not_fall_back_to_the_spa_entry() {
    // A hashed asset that is not on disk is a stale cache or a bad build; a
    // 200 with HTML in it would break the module loader instead of saying so.
    for uri in ["/assets/index-gone.js", "/user_uploads/42/missing.png"] {
        let (_ws, _dist, router) = console_router();
        let (status, _, _) = get(router, uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should be a 404");
    }
}

#[tokio::test]
async fn api_paths_are_never_served_from_the_console_directory() {
    // `dist/api/secret.json` exists on disk, but `/api/...` belongs to the hub
    // API and its 404 must not depend on what a build happened to emit.
    let (_ws, _dist, router) = console_router();
    let (status, _, _) = get(router, "/api/secret.json").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn traversal_out_of_the_console_directory_is_refused() {
    let workspace = tempfile::tempdir().unwrap();
    let outer = tempfile::tempdir().unwrap();
    std::fs::write(outer.path().join("secret.txt"), "top secret").unwrap();
    let dist = outer.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    fake_dist(&dist);

    for uri in [
        "/../secret.txt",
        "/assets/../../secret.txt",
        "/..%2fsecret.txt",
    ] {
        let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
        let config = HubConfig {
            console_dir: Some(dist.clone()),
            ..HubConfig::default()
        };
        let router = crate::hub::build_router_with_config(app, config);
        let (status, _, body) = get(router, uri).await;
        assert_ne!(status, StatusCode::OK, "{uri} must not be served");
        assert!(!body.contains("top secret"), "{uri} leaked: {body}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_pointing_outside_the_console_directory_is_refused() {
    let workspace = tempfile::tempdir().unwrap();
    let outer = tempfile::tempdir().unwrap();
    std::fs::write(outer.path().join("secret.txt"), "top secret").unwrap();
    let dist = outer.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    fake_dist(&dist);
    std::os::unix::fs::symlink(outer.path().join("secret.txt"), dist.join("leak.txt")).unwrap();

    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    let config = HubConfig {
        console_dir: Some(dist.clone()),
        ..HubConfig::default()
    };
    let router = crate::hub::build_router_with_config(app, config);
    let (status, _, body) = get(router, "/leak.txt").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("top secret"), "symlink leaked: {body}");
}

#[tokio::test]
async fn api_routes_still_work_when_the_console_is_served() {
    let (_ws, _dist, router) = console_router();
    let (status, _, _) = get(router, "/api/chambers").await;
    assert_eq!(status, StatusCode::OK);
}

#[test]
fn an_unset_console_dir_means_the_embedded_console() {
    // The usual install configures nothing: the console ships inside the binary.
    assert!(matches!(
        HubConfig::default().console_source(),
        ConsoleSource::Embedded
    ));
}

#[test]
fn a_set_console_dir_is_the_single_override() {
    let dir = std::path::PathBuf::from("/srv/console");
    let cfg = HubConfig {
        console_dir: Some(dir.clone()),
        ..HubConfig::default()
    };
    assert!(matches!(cfg.console_source(), ConsoleSource::Dir(p) if p == dir));
}

#[test]
fn a_dir_source_reads_contained_files_and_reports_its_index() {
    let dist = tempfile::tempdir().unwrap();
    fake_dist(dist.path());
    let source = ConsoleSource::Dir(dist.path().to_path_buf());
    assert!(source.has_index());
    let file = source.get("assets/index-abc123.js").unwrap();
    assert_eq!(&*file.bytes, b"export const x = 1;");
    assert_eq!(file.name, "assets/index-abc123.js");
    assert!(!file.etag.is_empty());
    assert!(source.get("assets/index-gone.js").is_none());
    assert!(source.get("../secret.txt").is_none());
    let empty = tempfile::tempdir().unwrap();
    assert!(!ConsoleSource::Dir(empty.path().to_path_buf()).has_index());
}

#[test]
fn describe_names_the_source_and_whether_a_build_is_there() {
    assert_eq!(ConsoleSource::Embedded.describe(), "embedded");
    let dist = tempfile::tempdir().unwrap();
    fake_dist(dist.path());
    assert_eq!(
        ConsoleSource::Dir(dist.path().to_path_buf()).describe(),
        format!("{} (present)", dist.path().display())
    );
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        ConsoleSource::Dir(empty.path().to_path_buf()).describe(),
        format!("{} (missing)", empty.path().display())
    );
}

/// The other half of the per-source copy. A binary built without the console
/// has no directory to name and a stale `console_dir` is not the cause, so the
/// page must blame the build rather than send the operator hunting in a path.
#[tokio::test]
async fn the_embedded_setup_page_blames_the_build_not_a_directory() {
    let resp = not_installed(&ConsoleSource::Embedded);
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        body.contains("built without the Agent Console"),
        "the embedded page must blame the build; body was {body}"
    );
    assert!(
        body.contains("make console-build"),
        "the embedded page must name the fix; body was {body}"
    );
    assert!(
        !body.contains("serving its console from"),
        "the embedded page must not claim to be reading a directory; body was {body}"
    );
}

#[tokio::test]
async fn a_hub_with_no_console_installed_says_so_instead_of_404ing() {
    // The failure this replaces: a hub serving a console directory that was
    // moved or never built answered every page with a bare 404 while looking
    // perfectly healthy, which reads as a broken hub rather than a missing
    // build.
    let workspace = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    let config = HubConfig {
        console_dir: Some(empty.path().to_path_buf()),
        ..HubConfig::default()
    };
    let router = crate::hub::build_router_with_config(app, config);

    let (status, ctype, body) = get(router.clone(), "/").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(ctype.starts_with("text/html"), "content-type was {ctype}");
    assert!(
        body.contains("make console-build") && body.contains("console_dir"),
        "the page must name both fixes; body was {body}"
    );
    assert!(
        body.contains(&empty.path().display().to_string()),
        "an override page must name the directory it looked in; body was {body}"
    );

    // A missing hashed asset stays a 404: answering it with HTML would break
    // the module loader instead of reporting the bad build.
    let (status, _, _) = get(router.clone(), "/assets/index-abc123.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The API is untouched by any of this.
    let (status, _, _) = get(router, "/api/chambers").await;
    assert_eq!(status, StatusCode::OK);
}

#[test]
fn spa_fallback_covers_extensionless_non_api_paths_only() {
    assert!(is_spa_route("/"));
    assert!(is_spa_route("/c/alpha"));
    assert!(is_spa_route("/user_uploads/42/report"));
    // A dotted last segment is a file request; a missing file must 404.
    assert!(!is_spa_route("/assets/index-abc123.js"));
    assert!(!is_spa_route("/favicon.ico"));
    assert!(!is_spa_route("/foo.js"));
    // Hashed asset names contain dots only in the extension, but the guard
    // keys on `/assets` too so a directory listing can never become the SPA.
    assert!(!is_spa_route("/assets/nested"));
    assert!(!is_spa_route("/api"));
    assert!(!is_spa_route("/api/chambers"));
    // Segment-exact, like the auth classifier: `/apiary` is an ordinary path.
    assert!(is_spa_route("/apiary"));
}

/// Like [`get`], but keeps the whole response so a test can read its headers
/// and send request headers of its own.
async fn request(
    router: axum::Router,
    method: &str,
    uri: &str,
    extra: &[(&str, &str)],
) -> axum::http::Response<Body> {
    let mut req = Request::builder().method(method).uri(uri);
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    router
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn header<'a>(resp: &'a axum::http::Response<Body>, name: &str) -> &'a str {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

#[tokio::test]
async fn hashed_assets_are_immutable_and_everything_else_is_no_cache() {
    let (_ws, _dist, router) = console_router();
    let asset = request(router.clone(), "GET", "/assets/index-abc123.js", &[]).await;
    assert_eq!(
        header(&asset, "cache-control"),
        "public, max-age=31536000, immutable"
    );
    for uri in ["/", "/c/alpha", "/sw.js", "/manifest.webmanifest"] {
        let resp = request(router.clone(), "GET", uri, &[]).await;
        assert_eq!(header(&resp, "cache-control"), "no-cache", "{uri}");
    }
}

#[tokio::test]
async fn a_matching_if_none_match_answers_304_without_a_body() {
    let (_ws, _dist, router) = console_router();
    let first = request(router.clone(), "GET", "/assets/index-abc123.js", &[]).await;
    let etag = header(&first, "etag").to_string();
    assert!(!etag.is_empty(), "served files carry an ETag");
    let second = request(
        router.clone(),
        "GET",
        "/assets/index-abc123.js",
        &[("if-none-match", etag.as_str())],
    )
    .await;
    assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(header(&second, "etag"), etag);
    let bytes = axum::body::to_bytes(second.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(bytes.is_empty());
    let stale = request(
        router,
        "GET",
        "/assets/index-abc123.js",
        &[("if-none-match", "\"nope\"")],
    )
    .await;
    assert_eq!(stale.status(), StatusCode::OK);
}

#[tokio::test]
async fn non_get_methods_are_405_on_the_page_surface() {
    for method in ["POST", "PUT", "DELETE"] {
        let (_ws, _dist, router) = console_router();
        let resp = request(router, method, "/", &[]).await;
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(header(&resp, "allow"), "GET, HEAD");
    }
    let (_ws, _dist, router) = console_router();
    let resp = request(router, "HEAD", "/", &[]).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn spa_classification_uses_the_decoded_path() {
    // `%2E` is a dot: this names a file, and a missing file is a 404, not the
    // SPA entry (which would break a module loader asking for a script).
    let (_ws, _dist, router) = console_router();
    let (status, _, _) = get(router, "/foo%2Ejs").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_ownership_outranks_the_method_guard() {
    // `/api` belongs to the hub API for *every* method: an unrouted API path
    // is a missing endpoint (404), not a page surface refusing to be written
    // to (405). Only non-API paths answer 405.
    let (_ws, _dist, router) = console_router();
    let api = request(router.clone(), "POST", "/api/nope", &[]).await;
    assert_eq!(api.status(), StatusCode::NOT_FOUND);
    let encoded = request(router.clone(), "POST", "/%61pi/nope", &[]).await;
    assert_eq!(encoded.status(), StatusCode::NOT_FOUND);
    let page = request(router, "POST", "/c/alpha", &[]).await;
    assert_eq!(page.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(header(&page, "allow"), "GET, HEAD");
}

#[test]
fn if_none_match_accepts_the_wildcard_and_lists_in_either_strength() {
    assert!(etag_matches("*", "\"abc\""));
    assert!(etag_matches(" * ", "W/\"12-34\""));
    // A browser revalidating several cached variants sends a list.
    assert!(etag_matches("\"other\", \"abc\"", "\"abc\""));
    assert!(etag_matches("W/\"other\", W/\"12-34\"", "W/\"12-34\""));
    // Weak comparison: the `W/` prefix is not part of the identity, so a tag
    // survives a proxy that added or dropped it.
    assert!(etag_matches("W/\"abc\"", "\"abc\""));
    assert!(etag_matches("\"12-34\"", "W/\"12-34\""));
    assert!(!etag_matches("\"nope\"", "\"abc\""));
    assert!(!etag_matches("\"nope\", \"neither\"", "\"abc\""));
    assert!(!etag_matches("", "\"abc\""));
}

#[tokio::test]
async fn every_response_carries_nosniff_and_a_no_referrer_policy() {
    let (_ws, _dist, router) = console_router();
    for uri in ["/", "/assets/index-abc123.js", "/api/chambers", "/nope.png"] {
        let resp = request(router.clone(), "GET", uri, &[]).await;
        assert_eq!(header(&resp, "x-content-type-options"), "nosniff", "{uri}");
        assert_eq!(header(&resp, "referrer-policy"), "no-referrer", "{uri}");
    }
}

#[tokio::test]
async fn console_html_carries_the_csp_and_assets_do_not() {
    let (_ws, _dist, router) = console_router();
    for uri in ["/", "/c/alpha"] {
        let resp = request(router.clone(), "GET", uri, &[]).await;
        assert_eq!(
            header(&resp, "content-security-policy"),
            CONSOLE_CSP,
            "{uri}"
        );
    }
    let asset = request(router.clone(), "GET", "/assets/index-abc123.js", &[]).await;
    assert_eq!(header(&asset, "content-security-policy"), "");

    // The setup page is HTML too.
    let workspace = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    let config = HubConfig {
        console_dir: Some(empty.path().to_path_buf()),
        ..HubConfig::default()
    };
    let router = crate::hub::build_router_with_config(app, config);
    let resp = request(router, "GET", "/", &[]).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(header(&resp, "content-security-policy"), CONSOLE_CSP);
}
