//! Chamber attachments over HTTP: uploads land in
//! `<chamber>/messages/attachments/` (where chat-bridge also materializes
//! platform attachments) and are served back with a containment check that
//! never lets a request name escape that directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde_json::json;

use crate::hub::state::AppState;

pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

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
pub async fn post_upload(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (chamber, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        if field.name() != Some("file") {
            continue;
        }
        let original = field.file_name().unwrap_or("attachment").to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        let stored = format!("{}_{}", sha12(&bytes), safe_name(&original));
        let dir = attachments_dir(&chamber);
        std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        std::fs::write(dir.join(&stored), &bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let url = format!("/api/chambers/{}/files/{}", entry.id, stored);
        return Ok(Json(json!({
            "ok": true,
            "name": stored,
            "markdown": format!("[{original}]({url})"),
        })));
    }
    Err(StatusCode::BAD_REQUEST)
}

fn mime_for(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain; charset=utf-8",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// `GET /api/chambers/{id}/files/{name}` — serve a stored attachment.
///
/// `{name}` must be exactly one already-sanitized segment. Anything that
/// could escape the attachments directory (separators, a leading dot, or any
/// character `safe_name` would have rewritten) is rejected *before* the path
/// is joined, so no traversal ever reaches the filesystem.
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
    let path = attachments_dir(&chamber).join(&name);
    let bytes = std::fs::read(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok((
        [
            (header::CONTENT_TYPE, mime_for(&name).to_string()),
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
