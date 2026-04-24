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
    pub agent_running: bool,
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
                agent_running: false,
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
        let overview = crate::chamber_status::overview(&entry.path);
        entry.running = overview.running;
        entry.agent_running = overview.agent_running;
        entry.session = overview.session;
        entry.next_wake = overview.next_wake;
        entry.next_wake_display = overview.next_wake_display;
        entry.wake_imminent = overview.wake_imminent;
        entry.unread = overview.unread;
        entry.task = overview.task;
        entry.last_message_preview = overview.last_message_preview;
        entry.completed = overview.completed;
        entry.sync = overview
            .sync
            .into_iter()
            .map(|badge| SyncBadge {
                backend: badge.backend,
                running: badge.running,
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

#[cfg(test)]
#[path = "../unit_tests/hub/discovery.rs"]
mod tests;
