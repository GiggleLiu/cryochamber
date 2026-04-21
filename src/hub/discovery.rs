//! Chamber discovery: scan `<dir>/*/cryo.toml`.
//!
//! Hub only surfaces chambers that live under the directory `cryohub` was
//! started in (the server's cwd). Daemons running elsewhere on the machine
//! (e.g. test leftovers under `/tmp/`) are intentionally not merged in —
//! they would clutter the rail and can't be managed from this hub instance
//! anyway.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Encode a canonicalized absolute path as a URL-safe chamber id.
pub fn encode_id(path: &Path) -> String {
    urlencoding::encode(&path.to_string_lossy()).into_owned()
}

/// Decode a chamber id back to an absolute path.
pub fn decode_id(id: &str) -> Option<PathBuf> {
    // Validate that all % sequences are valid hex pairs
    let mut chars = id.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex1 = chars.next()?;
            let hex2 = chars.next()?;
            if !hex1.is_ascii_hexdigit() || !hex2.is_ascii_hexdigit() {
                return None;
            }
        }
    }
    urlencoding::decode(id)
        .ok()
        .map(|s| PathBuf::from(s.into_owned()))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncBadge {
    pub backend: String,
    pub running: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChamberEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub config_error: Option<String>,
    pub running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub next_wake_display: Option<String>,
    pub wake_imminent: bool,
    pub unread: usize,
    pub task: Option<String>,
    pub last_message_preview: Option<String>,
    pub completed: bool,
    pub sync: Vec<SyncBadge>,
}

/// A map from chamber id → entry.
pub type ChamberIndex = BTreeMap<String, ChamberEntry>;

/// Scan `<dir>/*` for chambers. Returns entries for every subdirectory
/// (even ones with broken or missing `cryo.toml` — those get a
/// `config_error`). Runtime fields (`running`, `session`, `next_wake`,
/// `unread`) are filled in by `populate_runtime`, not here.
pub fn scan_workspace(dir: &Path) -> ChamberIndex {
    let mut out = ChamberIndex::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path.clone());
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(unknown)".into());
        let cryo_toml = canonical.join("cryo.toml");
        let config_error = if !cryo_toml.exists() {
            Some("missing cryo.toml".into())
        } else {
            crate::config::load_config(&cryo_toml)
                .err()
                .map(|e| e.to_string())
        };
        let id = encode_id(&canonical);
        out.insert(
            id.clone(),
            ChamberEntry {
                id,
                name,
                path: canonical,
                config_error,
                running: false,
                session: None,
                next_wake: None,
                next_wake_display: None,
                wake_imminent: false,
                unread: 0,
                task: None,
                last_message_preview: None,
                completed: false,
                sync: vec![],
            },
        );
    }
    out
}

/// Fill in runtime fields on each entry from its on-disk state.
pub fn populate_runtime(idx: &mut ChamberIndex) {
    for entry in idx.values_mut() {
        let dir = &entry.path;

        // Session # and running flag from timer.json
        if let Ok(Some(st)) = crate::state::load_state(&crate::state::state_path(dir)) {
            entry.session = Some(st.session_number);
            entry.running = crate::state::is_locked(&st);
        }

        // Next wake from todo.json
        let todo_path = dir.join("todo.json");
        entry.next_wake = crate::todo::TodoList::load(&todo_path)
            .ok()
            .and_then(|list| list.next_wake_time().map(String::from));
        entry.next_wake_display = entry.next_wake.clone();
        entry.wake_imminent = entry
            .next_wake
            .as_deref()
            .and_then(|w| chrono::NaiveDateTime::parse_from_str(w, "%Y-%m-%dT%H:%M").ok())
            .map(|wake| {
                let diff = wake - chrono::Local::now().naive_local();
                let diff_ms = diff.num_milliseconds();
                (0..=3_600_000).contains(&diff_ms)
            })
            .unwrap_or(false);

        // Unread = pending inbox messages (not archived)
        entry.unread = crate::message::read_inbox(dir)
            .map(|v| v.len())
            .unwrap_or(0);

        entry.task = crate::log::parse_latest_session_task(&crate::log::log_path(dir))
            .ok()
            .flatten();
        entry.last_message_preview = last_message_preview(dir);

        // Plan completion flag from the last session in cryo.log
        let log_file = crate::log::log_path(dir);
        entry.completed = crate::log::parse_latest_session_plan_complete(&log_file)
            .ok()
            .flatten()
            .is_some();

        // Sync summaries, compact badge form (full detail served by GET /sync)
        entry.sync = crate::sync_common::summarize_all(dir)
            .into_iter()
            .map(|s| SyncBadge {
                backend: s.backend.as_str().into(),
                running: s.running,
            })
            .collect();
    }
}

/// One-shot discovery: scan workspace and populate runtime fields.
pub fn discover(workspace: &Path) -> ChamberIndex {
    let mut idx = scan_workspace(workspace);
    populate_runtime(&mut idx);
    idx
}

fn last_message_preview(dir: &Path) -> Option<String> {
    let mut messages = Vec::new();
    if let Ok(archived) = crate::message::read_inbox_archive(dir) {
        messages.extend(archived);
    }
    if let Ok(inbox) = crate::message::read_inbox(dir) {
        messages.extend(inbox);
    }
    if let Ok(outbox) = crate::message::read_outbox(dir) {
        messages.extend(outbox);
    }
    if let Ok(archived) = crate::message::read_outbox_archive(dir) {
        messages.extend(archived);
    }
    messages
        .into_iter()
        .max_by(|(file_a, msg_a), (file_b, msg_b)| {
            msg_a
                .timestamp
                .cmp(&msg_b.timestamp)
                .then_with(|| file_a.cmp(file_b))
        })
        .and_then(|(_, msg)| preview_body(&msg.body))
}

fn preview_body(body: &str) -> Option<String> {
    let line = body.lines().find(|line| !line.trim().is_empty())?.trim();
    if line.chars().count() <= 120 {
        return Some(line.to_string());
    }
    Some(line.chars().take(117).collect::<String>() + "...")
}

#[cfg(test)]
#[path = "../unit_tests/hub/discovery.rs"]
mod tests;
