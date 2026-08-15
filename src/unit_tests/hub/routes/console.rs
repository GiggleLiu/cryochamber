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
    // The console routes `/c/...` and `/user_uploads/...` client-side, so a
    // deep link (or a reload on one) must land on `index.html`, not a 404.
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

#[tokio::test]
async fn without_console_dir_the_bundled_shell_still_answers() {
    // Regression guard for every existing deployment: no `console_dir`, no
    // change — `/`, `/c/{id}` and the bundled assets behave exactly as before.
    let workspace = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    let router = crate::hub::build_router_with_config(app, HubConfig::default());

    let (status, ctype, body) = get(router.clone(), "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ctype.starts_with("text/html"), "content-type was {ctype}");
    assert!(body.contains("<title>Cryohub</title>"), "body was {body}");

    let (status, ctype, _) = get(router.clone(), "/assets/web.css").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ctype.starts_with("text/css"), "content-type was {ctype}");

    // An unknown path is still a plain 404, not an SPA entry.
    let (status, _, _) = get(router, "/user_uploads/42/report").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn spa_fallback_covers_extensionless_non_api_paths_only() {
    assert!(is_spa_route("/"));
    assert!(is_spa_route("/c/alpha"));
    assert!(is_spa_route("/user_uploads/42/report"));
    // A dotted last segment is a file request; a missing file must 404.
    assert!(!is_spa_route("/assets/index-abc123.js"));
    assert!(!is_spa_route("/favicon.ico"));
    // Hashed asset names contain dots only in the extension, but the guard
    // keys on `/assets` too so a directory listing can never become the SPA.
    assert!(!is_spa_route("/assets/nested"));
    assert!(!is_spa_route("/api"));
    assert!(!is_spa_route("/api/chambers"));
    // Segment-exact, like the auth classifier: `/apiary` is an ordinary path.
    assert!(is_spa_route("/apiary"));
}
