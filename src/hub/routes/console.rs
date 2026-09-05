//! Serve the built Agent Console (a vite `dist/`) — embedded in the binary,
//! or read from an operator-configured `console_dir`.
//!
//! Wired as the router's only fallback, so it sees every request no hub route
//! claimed — the console *is* the hub's page surface. Four rules, in order:
//!
//! 1. `/api` and `/api/...` never touch the filesystem — the hub API owns that
//!    prefix, and its 404 must not depend on what a build happened to emit.
//! 2. A real file in the console source is served with its content type.
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
//! Every served file carries an `ETag` and a `Cache-Control`: hashed output
//! under `/assets/` is `immutable`, everything else is `no-cache`, so a deploy
//! reaches an already-installed client on its next load. A matching
//! `If-None-Match` answers `304` with no body.
//!
//! Every HTML this route emits — the SPA entry and the not-installed page —
//! carries [`CONSOLE_CSP`]. HTML is the only thing a browser executes, so it
//! is the only thing worth constraining; assets and JSON get the global
//! `nosniff` / `Referrer-Policy` from [`crate::hub::security`] and nothing
//! more.
//!
//! Only `GET` and `HEAD` reach any of this; other methods get `405` with
//! `Allow: GET, HEAD`, because a page surface has nothing to write to. Rule 1
//! outranks that guard: `/api` is the hub API's for every method, so an
//! unrouted API path is a `404` (a missing endpoint) rather than a `405`.
//!
//! An embedded lookup is a key lookup and cannot escape. A `console_dir`
//! lookup gets the same containment discipline as chamber attachments:
//! resolve first, then require the result to be under the canonicalized
//! root, so neither `../` nor a planted symlink can name a file outside it.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

use crate::hub::mime::mime_for;

/// Applied to every HTML the console route emits. Same-origin everything;
/// `unsafe-inline` styles because KaTeX emits inline `style=`; models.dev is
/// the provider catalog the New Chamber sheet fetches; `data:` fonts because
/// Vite inlines the smallest KaTeX face as a data:font/woff2 URI.
pub const CONSOLE_CSP: &str = "default-src 'self'; frame-src blob:; img-src 'self' data: blob:; \
    connect-src 'self' https://models.dev; style-src 'self' 'unsafe-inline'; \
    font-src 'self' data:; frame-ancestors 'none'; base-uri 'none'";

/// The built console compiled into the binary. `console/dist/` is created by
/// `build.rs` when absent, so a checkout without Node still compiles; the
/// release pipeline builds the console first so the published crate embeds it.
#[derive(rust_embed::Embed)]
#[folder = "console/dist/"]
struct ConsoleDist;

/// Where the console comes from: the binary, or an operator-supplied build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleSource {
    Embedded,
    Dir(PathBuf),
}

/// One file the console route can answer with.
pub struct ConsoleFile {
    pub bytes: Cow<'static, [u8]>,
    /// A strong tag for embedded files (content sha256); a weak
    /// `W/"len-mtime"` tag for on-disk files.
    pub etag: String,
    /// The URL-relative name, used for the content type.
    pub name: String,
}

impl ConsoleSource {
    /// `rel` is a URL path with the leading `/` stripped and percent-decoded.
    pub fn get(&self, rel: &str) -> Option<ConsoleFile> {
        match self {
            Self::Embedded => {
                let file = ConsoleDist::get(rel)?;
                let etag = format!(
                    "\"{}\"",
                    file.metadata
                        .sha256_hash()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                );
                Some(ConsoleFile {
                    bytes: file.data,
                    etag,
                    name: rel.to_string(),
                })
            }
            Self::Dir(root) => {
                let path = contained_file(root, rel)?;
                let meta = std::fs::metadata(&path).ok()?;
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let bytes = std::fs::read(&path).ok()?;
                Some(ConsoleFile {
                    etag: format!("W/\"{}-{mtime}\"", bytes.len()),
                    bytes: Cow::Owned(bytes),
                    name: rel.to_string(),
                })
            }
        }
    }

    /// Is there a console here at all? Decided per request so an operator can
    /// drop a build into `console_dir` without restarting the hub.
    pub fn has_index(&self) -> bool {
        match self {
            Self::Embedded => ConsoleDist::get("index.html").is_some(),
            Self::Dir(root) => contained_file(root, "index.html").is_some(),
        }
    }

    /// One line for `cryohub start`/`status`. An override says whether a build
    /// is actually there, so the operator learns about a mistyped or emptied
    /// `console_dir` from the terminal instead of from a 503 in the browser.
    pub fn describe(&self) -> String {
        match self {
            Self::Embedded => "embedded".to_string(),
            Self::Dir(root) => format!(
                "{} ({})",
                root.display(),
                if self.has_index() {
                    "present"
                } else {
                    "missing"
                }
            ),
        }
    }
}

/// The canonical path of `rel` inside `root`, if it really is a regular file
/// inside it. Resolve first, then require the result to be under the
/// canonicalized root, so neither `../` nor a planted symlink can name a file
/// outside the console directory.
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

/// Hashed build output may be cached forever; the entry points that name it
/// (`index.html`, `sw.js`, the manifest, `precache.json`) must be revalidated
/// on every load or a deploy would never reach an installed PWA.
fn cache_control_for(rel: &str) -> &'static str {
    if rel.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// Does `if_none_match` (the raw header value) name `etag`? Handles the `*`
/// wildcard and comma-separated lists; weak comparison, as GET allows.
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    let strip = |t: &str| t.trim().trim_start_matches("W/").to_string();
    if if_none_match.trim() == "*" {
        return true;
    }
    let want = strip(etag);
    if_none_match.split(',').any(|t| strip(t) == want)
}

fn serve_file(file: ConsoleFile, if_none_match: Option<&str>) -> Response {
    let cache = cache_control_for(&file.name);
    if let Some(inm) = if_none_match {
        if etag_matches(inm, &file.etag) {
            return (
                StatusCode::NOT_MODIFIED,
                [
                    (header::ETAG, file.etag.clone()),
                    (header::CACHE_CONTROL, cache.to_string()),
                ],
            )
                .into_response();
        }
    }
    let content_type = mime_for(&file.name);
    let mut resp = (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
            (header::ETAG, file.etag.clone()),
        ],
        file.bytes.into_owned(),
    )
        .into_response();
    // Only HTML executes anything, so only HTML needs the policy — putting it
    // on a script or an image would be noise the browser ignores.
    if content_type.starts_with("text/html") {
        resp.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            header::HeaderValue::from_static(CONSOLE_CSP),
        );
    }
    resp
}

/// One [`ConsoleSource::get`] off the async runtime. A join failure reads as
/// "no such file", which lands on the SPA entry or the not-installed page —
/// the same answer a genuinely missing file gets.
async fn blocking_get(source: &Arc<ConsoleSource>, rel: String) -> Option<ConsoleFile> {
    let source = source.clone();
    tokio::task::spawn_blocking(move || source.get(&rel))
        .await
        .ok()
        .flatten()
}

/// Router fallback: the console *is* the hub's page surface.
pub async fn serve(source: Arc<ConsoleSource>, req: Request) -> Response {
    let raw = req.uri().path().to_string();
    // Percent-decoding happens once, before containment and before the SPA
    // decision, so `%2e%2e%2f` is judged as the `../` it is and `%2E` as a
    // dot rather than as innocent literal characters.
    let rel = urlencoding::decode(raw.trim_start_matches('/'))
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| raw.trim_start_matches('/').to_string());
    let decoded = format!("/{rel}");
    // API ownership is decided before the method guard: an unrouted `/api`
    // path is a missing endpoint for every method, and answering it with a
    // page-surface 405 would misreport the hub API's shape.
    if is_api_path(&raw) || is_api_path(&decoded) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !matches!(
        *req.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    let if_none_match = req
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // A `Dir` source reads from disk, so every lookup — the file the URL names
    // and the SPA entry it falls back to — goes off the async runtime.
    let found = blocking_get(&source, rel.clone()).await;
    if let Some(file) = found {
        return serve_file(file, if_none_match.as_deref());
    }
    if !is_spa_route(&decoded) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match blocking_get(&source, "index.html".to_string()).await {
        Some(index) => serve_file(index, if_none_match.as_deref()),
        None => not_installed(&source),
    }
}

/// The page a hub shows when there is no console where it is looking.
/// Deliberately self-contained — no stylesheet, no script, nothing to fetch —
/// because everything that would serve those is the thing that is missing.
///
/// The two sources fail for different reasons and take different fixes, so the
/// copy names the one that applies: a binary built without the console needs a
/// console build and a reinstall, while an override that came up empty needs
/// the operator to look at the directory the page names.
fn not_installed(source: &ConsoleSource) -> Response {
    let where_ = match source {
        ConsoleSource::Embedded => "This cryohub was built without the Agent Console.".to_string(),
        ConsoleSource::Dir(root) => format!(
            "The hub is serving its console from <code>{}</code>, and there is no build there.",
            html_escape(&root.display().to_string())
        ),
    };
    let body = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Agent Console not available</title>\n</head>\n\
         <body style=\"margin:0;display:grid;place-items:center;min-height:100vh;\
         font:16px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;\
         color:#16171a;background:#f5f5f6\">\n\
         <main style=\"max-width:32rem;padding:2rem\">\n\
         <h1 style=\"font-size:1.25rem;margin:0 0 .5rem\">No Agent Console here</h1>\n\
         <p style=\"margin:0 0 1rem;color:#43474d\">{where_}</p>\n\
         <p style=\"margin:0 0 1rem;color:#43474d\">Build it and rebuild the binary:</p>\n\
         <pre style=\"padding:.75rem 1rem;background:#fff;border:1px solid #e3e4e6;\
         border-radius:8px;overflow-x:auto\">make console-build &amp;&amp; cargo install --path .</pre>\n\
         <p style=\"margin:0 0 1rem;color:#43474d\">Or point <code>console_dir</code> in \
         <code>cryohub.toml</code> at a built console (an absolute path).</p>\n\
         <p style=\"margin:1rem 0 0;color:#5f646a;font-size:.875rem\">The API is \
         unaffected — <code>/api/...</code> answers normally.</p>\n\
         </main>\n</body>\n</html>\n"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
            (header::CONTENT_SECURITY_POLICY, CONSOLE_CSP.to_string()),
        ],
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
