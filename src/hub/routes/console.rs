//! Serve the built Agent Console (a vite `dist/`) from disk.
//!
//! Wired as the router's only fallback, so it sees every request no hub route
//! claimed — the console *is* the hub's page surface. Four rules, in order:
//!
//! 1. `/api` and `/api/...` never touch the filesystem — the hub API owns that
//!    prefix, and its 404 must not depend on what a build happened to emit.
//! 2. A real file under the console root is served with its content type.
//! 3. Anything else that could be a client-side route (no extension on the
//!    last segment, outside `/assets`) gets `index.html`, so a deep link into
//!    `/c/...` or `/user_uploads/...` survives a reload. A *missing file*
//!    stays a 404: answering a stale hashed asset with HTML would break the
//!    module loader instead of reporting the bad build.
//! 4. If there is no `index.html` at all, the console is not installed. That is
//!    a setup state, not a missing page, so it answers with the reason and the
//!    command that fixes it rather than a bare 404 from a hub that otherwise
//!    looks healthy.
//!
//! Containment is the same discipline as chamber attachments: resolve first,
//! then require the result to be under the canonicalized root, so neither
//! `../` nor a planted symlink can name a file outside the console directory.

use std::path::{Path, PathBuf};

use axum::{
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

use crate::hub::mime::mime_for;

/// The canonical path of `rel` inside `root`, if it really is a regular file
/// inside it. `rel` is a URL path with the leading `/` already stripped.
fn contained_file(root: &Path, rel: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let resolved = root.join(rel).canonicalize().ok()?;
    if !resolved.starts_with(&root) {
        return None;
    }
    resolved.is_file().then_some(resolved)
}

/// Does the hub API own this path? Segment-exact, like
/// [`crate::hub::auth::classify`], so `/apiary` is an ordinary console route.
fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// Is `path` a plausible client-side route — something the SPA entry should
/// answer rather than a file lookup that missed?
///
/// A dot in the last segment means a file was asked for by name, and `/assets`
/// holds nothing but build output, so neither may become the SPA.
fn is_spa_route(path: &str) -> bool {
    if is_api_path(path) || path == "/assets" || path.starts_with("/assets/") {
        return false;
    }
    !path.rsplit('/').next().unwrap_or("").contains('.')
}

/// Read a file and answer with it, or 404 if it vanished between the
/// containment check and the read.
async fn serve_file(path: PathBuf, name: String) -> Response {
    let Ok(Some(bytes)) = tokio::task::spawn_blocking(move || std::fs::read(&path).ok()).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    ([(header::CONTENT_TYPE, mime_for(&name))], bytes).into_response()
}

/// Router fallback for a configured console directory.
pub async fn serve(root: PathBuf, req: Request) -> Response {
    let path = req.uri().path().to_string();
    if is_api_path(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    // Percent-decoding happens once, before containment, so `%2e%2e%2f` is
    // judged as the `../` it is rather than as an innocent literal segment.
    let rel = urlencoding::decode(path.trim_start_matches('/'))
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.trim_start_matches('/').to_string());

    let lookup_root = root.clone();
    let lookup_rel = rel.clone();
    let found = tokio::task::spawn_blocking(move || contained_file(&lookup_root, &lookup_rel))
        .await
        .ok()
        .flatten();
    if let Some(file) = found {
        return serve_file(file, rel).await;
    }
    if !is_spa_route(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match contained_file(&root, "index.html") {
        Some(index) => serve_file(index, "index.html".to_string()).await,
        None => not_installed(&root),
    }
}

/// The page a hub shows when no console is installed where it is looking.
/// Deliberately self-contained — no stylesheet, no script, nothing to fetch —
/// because everything that would serve those is the thing that is missing.
fn not_installed(root: &Path) -> Response {
    let body = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Agent Console not installed</title>\n</head>\n\
         <body style=\"margin:0;display:grid;place-items:center;min-height:100vh;\
         font:16px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;\
         color:#16171a;background:#f5f5f6\">\n\
         <main style=\"max-width:32rem;padding:2rem\">\n\
         <h1 style=\"font-size:1.25rem;margin:0 0 .5rem\">No Agent Console here</h1>\n\
         <p style=\"margin:0 0 1rem;color:#43474d\">The hub is running. It serves its \
         dashboard from <code>{}</code>, and there is no build there yet.</p>\n\
         <p style=\"margin:0 0 1rem;color:#43474d\">From a cryochamber checkout:</p>\n\
         <pre style=\"padding:.75rem 1rem;background:#fff;border:1px solid #e3e4e6;\
         border-radius:8px;overflow-x:auto\">make console-install</pre>\n\
         <p style=\"margin:1rem 0 0;color:#5f646a;font-size:.875rem\">The API is \
         unaffected — <code>/api/...</code> answers normally.</p>\n\
         </main>\n</body>\n</html>\n",
        html_escape(&root.display().to_string()),
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

/// The console root is operator-controlled, not attacker-controlled, but it is
/// interpolated into HTML — escape it rather than reason about who can set it.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/console.rs"]
mod tests;
