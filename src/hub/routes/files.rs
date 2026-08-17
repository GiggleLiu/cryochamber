//! Chamber attachments over HTTP: uploads land in
//! `<chamber>/messages/attachments/` (where chat-bridge also materializes
//! platform attachments) and are served back with a containment check that
//! never lets a request name escape that directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::hub::mime::mime_for;
use crate::hub::state::AppState;

pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

/// How much a single chamber's attachments directory may hold before further
/// uploads are refused with 507. The per-file cap bounds one request; this
/// bounds the total a leaked invite can accumulate.
pub const MAX_ATTACHMENTS_DIR_BYTES: u64 = 200 * 1024 * 1024;

/// How many uploads may be handled at once, process-wide.
///
/// Every upload buffers up to 25 MB and then touches the disk. Without a bound,
/// a handful of parallel requests from one authenticated client can pin memory
/// and stall the runtime — the per-request body limit does not constrain
/// aggregate use.
const MAX_CONCURRENT_UPLOADS: usize = 2;

fn upload_slots() -> &'static tokio::sync::Semaphore {
    static SLOTS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| tokio::sync::Semaphore::new(MAX_CONCURRENT_UPLOADS))
}

/// Reduce a client-supplied filename to a single safe path segment: keep
/// `[A-Za-z0-9._-]`, replace everything else with `_`, strip leading dots
/// (no dotfiles, no `..`). Never returns an empty string.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "attachment".into()
    } else {
        cleaned
    }
}

fn attachments_dir(chamber: &Path) -> PathBuf {
    chamber.join("messages").join("attachments")
}

/// The canonicalized attachments directory, or `None` if it is missing, is
/// itself a symlink, or resolves outside the chamber.
///
/// Same shape as [`crate::channel::zulip::resolve_local_attachment`]: resolve
/// first, then require containment. `symlink_metadata` is what catches a
/// swapped-out directory — `canonicalize` alone would cheerfully follow it and
/// then report a perfectly "contained" path outside the chamber.
fn contained_attachments_dir(chamber: &Path) -> Option<PathBuf> {
    let dir = attachments_dir(chamber);
    if std::fs::symlink_metadata(&dir)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return None;
    }
    let root = chamber.canonicalize().ok()?;
    let resolved = dir.canonicalize().ok()?;
    resolved.starts_with(&root).then_some(resolved)
}

/// The canonical path of `name` inside the (already canonicalized) attachments
/// directory, if it really is a regular file inside it. A symlink pointing at
/// `../../cryo.toml` resolves outside `root` and is rejected here.
fn contained_file(root: &Path, name: &str) -> Option<PathBuf> {
    let resolved = root.join(name).canonicalize().ok()?;
    if !resolved.starts_with(root) {
        return None;
    }
    resolved.is_file().then_some(resolved)
}

/// Apparent size of the attachments directory. Attachments are stored flat, so
/// this is a single non-recursive pass; `DirEntry::metadata` does not follow
/// symlinks, so a planted link cannot inflate or hide the total.
fn attachments_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Write `bytes` to `<chamber>/messages/attachments/<stored>`.
///
/// `create_new` never follows a symlink and never truncates an existing entry,
/// so a predictable name planted in advance cannot be written through. A
/// collision is not an error: the name carries the content hash, so what is
/// already there is what is being uploaded — but it still has to be a regular
/// file inside the directory before its link is handed back.
fn store_attachment(chamber: &Path, stored: &str, bytes: &[u8]) -> Result<(), StatusCode> {
    use std::io::Write;

    let dir = attachments_dir(chamber);
    std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let root = contained_attachments_dir(chamber).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(stored))
    {
        Ok(mut file) => file
            .write_all(bytes)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => contained_file(&root, stored)
            .map(|_| ())
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn sha12(bytes: &[u8]) -> String {
    // No sha2 dependency: FNV-1a folded twice is enough for a collision-
    // avoiding storage prefix (not a security boundary — names are served
    // only from the attachments dir).
    let mut h1: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h1 ^= *b as u64;
        h1 = h1.wrapping_mul(0x100000001b3);
    }
    let mut h2: u64 = 0xcbf29ce484222325;
    for b in bytes.iter().rev() {
        h2 ^= *b as u64;
        h2 = h2.wrapping_mul(0x100000001b3);
    }
    format!("{h1:08x}{h2:08x}")[..12].to_string()
}

/// `POST /api/chambers/{id}/uploads` — multipart field `file`.
///
/// The stored name is `<hash12>_<sanitized>` so two different files never
/// collide and no client-chosen name reaches the filesystem verbatim.
/// Returns the markdown snippet the composer pastes into a message.
///
/// Three bounds apply, in the order they can be checked most cheaply:
/// a process-wide concurrency permit, the chamber's storage quota (507), and
/// the per-file size cap (413). Every filesystem touch runs on a blocking
/// worker so a slow disk cannot stall the async runtime.
pub async fn post_upload(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    role: Option<axum::Extension<crate::hub::tokens::Role>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, Response> {
    let (chamber, entry) = app
        .resolve(&id)
        .ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;
    // Same bucket as `send`: an upload is the other way a guest writes to the
    // owner's disk, so a flood of either must run the credential dry.
    if let Some(throttled) = app.write_limiter.refuse(role.as_ref().map(|e| &e.0)) {
        return Err(throttled);
    }
    let _slot = upload_slots()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

    // Quota before the body: no point buffering 25 MB that will be refused.
    let quota_chamber = chamber.clone();
    let used =
        tokio::task::spawn_blocking(move || attachments_bytes(&attachments_dir(&quota_chamber)))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;
    if used >= MAX_ATTACHMENTS_DIR_BYTES {
        return Err(StatusCode::INSUFFICIENT_STORAGE.into_response());
    }

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST.into_response())?
    {
        if field.name() != Some("file") {
            continue;
        }
        let original = field.file_name().unwrap_or("attachment").to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE.into_response())?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE.into_response());
        }
        let stored = format!("{}_{}", sha12(&bytes), safe_name(&original));
        let write_chamber = chamber.clone();
        let write_stored = stored.clone();
        tokio::task::spawn_blocking(move || {
            store_attachment(&write_chamber, &write_stored, &bytes)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?
        .map_err(IntoResponse::into_response)?;
        let url = format!("/api/chambers/{}/files/{}", entry.id, stored);
        return Ok(Json(json!({
            "ok": true,
            "name": stored,
            "markdown": format!("[{original}]({url})"),
        })));
    }
    Err(StatusCode::BAD_REQUEST.into_response())
}

/// Attachments are downloads, never documents: anything a browser could run
/// or render as markup goes out as an opaque byte stream, on top of the
/// `Content-Disposition: attachment` every attachment already carries.
fn attachment_content_type(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" | "xhtml" | "js" | "mjs" | "svg" => "application/octet-stream",
        _ => mime_for(name),
    }
}

/// `GET /api/chambers/{id}/files/{name}` — serve a stored attachment.
///
/// `{name}` must be exactly one already-sanitized segment. Anything that
/// could escape the attachments directory (separators, a leading dot, or any
/// character `safe_name` would have rewritten) is rejected *before* the path
/// is joined, so no traversal ever reaches the filesystem.
///
/// A legal name can still *point* out of the directory, so the resolved path
/// is checked for containment too — a symlinked entry, or a symlinked
/// attachments directory, 404s rather than serving whatever it aims at.
pub async fn get_file(
    State(app): State<Arc<AppState>>,
    AxumPath((id, name)): AxumPath<(String, String)>,
) -> Result<Response, StatusCode> {
    let (chamber, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    // Containment: exactly one sanitized segment, no separators, no dotfiles.
    if name.contains('/')
        || name.contains('\\')
        || name.starts_with('.')
        || name != safe_name(&name)
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let read_name = name.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        let root = contained_attachments_dir(&chamber)?;
        std::fs::read(contained_file(&root, &read_name)?).ok()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                attachment_content_type(&name).to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Top-level directories whose files may be shared through the console.
/// These are the research artifacts the agent produces on disk (reports in
/// `articles/`, references in `.knowledge/`) and links from messages. The
/// mailbox, `.cryo/` credentials/state, config and notes stay off the wire
/// even though they are chamber-contained.
const SHAREABLE_DIRS: &[&str] = &["articles", ".knowledge"];

/// Validate a chamber-relative path for the share endpoint: not absolute, no
/// backslashes, no NUL, and no `.`/`..`/empty path components. Containment is
/// still re-checked against the canonicalized chamber root before serving, so
/// this is the cheap first gate, not the boundary.
fn safe_rel_path(rel: &str) -> bool {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') || rel.contains('\0') {
        return false;
    }
    rel.split('/')
        .all(|c| !c.is_empty() && c != "." && c != "..")
}

/// Filename safe to interpolate into a quoted `Content-Disposition` header:
/// quotes and control characters are dropped (they could otherwise break out
/// of the quoted-string or smuggle header lines). Chamber-file names are not
/// sanitized like attachment names, so this is the boundary for them.
fn disposition_filename(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '"' && !c.is_control())
        .collect()
}

#[derive(Deserialize)]
pub struct ChamberFileQuery {
    path: String,
}

/// `GET /api/chambers/{id}/file?path=articles/review.pdf` — serve a
/// chamber-local research artifact (a report or reference the agent linked in
/// a message). Unlike attachments, these are produced on disk by the agent
/// (e.g. a Typst PDF) rather than uploaded through the console, so they live
/// outside `messages/attachments/`.
///
/// The `path` is chamber-relative. It is validated component-wise, resolved
/// against the canonical chamber root, and must still be contained inside it
/// and under a shareable top-level directory. A symlink that resolves outside
/// the chamber — or a path that escapes `articles/`/`.knowledge/` — 404s.
pub async fn get_chamber_file(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<ChamberFileQuery>,
) -> Result<Response, StatusCode> {
    let path = query.path;
    let (chamber, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    if !safe_rel_path(&path) {
        return Err(StatusCode::NOT_FOUND);
    }
    let read_path = path.clone();
    let name = disposition_filename(path.rsplit('/').next().unwrap_or(&path));
    let bytes = tokio::task::spawn_blocking(move || {
        let root = chamber.canonicalize().ok()?;
        let resolved = root.join(&read_path).canonicalize().ok()?;
        if !resolved.starts_with(&root) {
            return None;
        }
        let top = resolved
            .strip_prefix(&root)
            .ok()?
            .components()
            .next()?
            .as_os_str()
            .to_str()?
            .to_string();
        if !SHAREABLE_DIRS.contains(&top.as_str()) {
            return None;
        }
        // Only regular files. Note: a *hard link* inside a shareable dir to a
        // file elsewhere in the chamber is indistinguishable by path and would
        // be served. That requires local write access to the chamber — the
        // same privilege that can read any chamber file directly — so it is
        // accepted; the remote-reachable vector (a symlink planted over HTTP)
        // is rejected by the canonicalize+containment check above.
        std::fs::metadata(&resolved).ok().filter(|m| m.is_file())?;
        std::fs::read(&resolved).ok()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                attachment_content_type(&name).to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/files.rs"]
mod tests;
