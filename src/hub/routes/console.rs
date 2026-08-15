//! Serve the built Agent Console (a vite `dist/`) from disk.
//!
//! Wired as the router's fallback when `console_dir` is configured, so it only
//! sees requests no hub route claimed. Three rules, in order:
//!
//! 1. `/api` and `/api/...` never touch the filesystem — the hub API owns that
//!    prefix, and its 404 must not depend on what a build happened to emit.
//! 2. A real file under `console_dir` is served with its content type.
//! 3. Anything else that could be a client-side route (no extension on the
//!    last segment, outside `/assets`) gets `index.html`, so a deep link into
//!    `/c/...` or `/user_uploads/...` survives a reload. A *missing file*
//!    stays a 404: answering a stale hashed asset with HTML would break the
//!    module loader instead of reporting the bad build.
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
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/console.rs"]
mod tests;
