//! Chamber discovery for Cryohub.
//!
//! Product discovery is registry-only: Cryohub shows chambers remembered by
//! `cryo start` or created through the hub. Workspace scanning remains as a
//! test/helper mode for route tests that need isolated chamber indexes.

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

/// Reduce a chamber reference to the one form the index keys on: the
/// percent-encoded id produced by [`encode_id`].
///
/// A chamber may be named in two ways — the encoded index id
/// (`%2Fhome%2Fme%2Falpha`) or the plain decoded path (`/home/me/alpha`) — and
/// invite scopes are written by hand on the CLI, so both reach the server.
/// Every scope comparison runs through this function, so an invite cannot end
/// up passing the route guard while its chamber is missing from the list and
/// its SSE stream.
///
/// Inputs that are neither (a bare name like `alpha`, used in unit tests) map
/// to themselves: `encode_id` only escapes characters a plain name lacks.
pub fn canonical_scope(reference: &str) -> String {
    if let Some(path) = decode_id(reference) {
        let re_encoded = encode_id(&path);
        // Already canonical: decoding then re-encoding is the identity.
        if re_encoded == reference {
            return re_encoded;
        }
    }
    encode_id(Path::new(reference))
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
    pub path_hint: Option<String>,
    pub config_error: Option<String>,
    pub running: bool,
    pub agent_running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub next_wake_display: Option<String>,
    pub wake_imminent: bool,
    pub has_open_question: bool,
    pub task: Option<String>,
    pub last_message_preview: Option<String>,
    pub completed: bool,
    /// Hub display state carried from the registry: archived chambers fold into
    /// the dashboard's "Archived" section and cannot be launched until
    /// unarchived. Not a runtime field — `populate_runtime` leaves it untouched.
    pub archived: bool,
    pub sync: Vec<SyncBadge>,
}

/// A map from chamber id → entry.
pub type ChamberIndex = BTreeMap<String, ChamberEntry>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryOptions {
    pub scan_workspace: bool,
    pub include_registry: bool,
}

impl DiscoveryOptions {
    pub fn all_chambers() -> Self {
        Self {
            scan_workspace: false,
            include_registry: true,
        }
    }

    pub fn local_only() -> Self {
        Self {
            scan_workspace: true,
            include_registry: false,
        }
    }
}

/// Scan `<dir>/*` for chambers. A subdirectory is treated as a chamber
/// only if it contains a `cryo.toml`; chambers with a malformed
/// `cryo.toml` are still listed and tagged with `config_error` so the
/// operator can see what's broken. Subdirectories without any
/// `cryo.toml` (e.g. a stray `messages/` or unrelated folder) are
/// skipped entirely. Runtime fields (`running`, `session`, `next_wake`,
/// `has_open_question`) are filled in by `populate_runtime`, not here.
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
        let cryo_toml = canonical.join("cryo.toml");
        if !cryo_toml.exists() {
            continue;
        }
        let name = chamber_display_name(&canonical);
        let config_error = crate::config::load_config(&cryo_toml)
            .err()
            .map(|e| e.to_string());
        let id = encode_id(&canonical);
        out.insert(
            id.clone(),
            ChamberEntry {
                id,
                name,
                path: canonical,
                path_hint: None,
                config_error,
                running: false,
                agent_running: false,
                session: None,
                next_wake: None,
                next_wake_display: None,
                wake_imminent: false,
                has_open_question: false,
                task: None,
                last_message_preview: None,
                completed: false,
                archived: false,
                sync: vec![],
            },
        );
    }
    out
}

fn chamber_display_name(path: &Path) -> String {
    if path.file_name().is_some_and(|name| name == ".cryo") {
        if let Some(parent_name) = path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().into_owned())
        {
            return parent_name;
        }
    }
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "(unknown)".into())
}

fn registry_entry_for_path(workspace: &Path, path: PathBuf) -> Option<ChamberEntry> {
    let canonical = path.canonicalize().unwrap_or(path);
    let cryo_toml = crate::config::config_path(&canonical);
    if !cryo_toml.exists() {
        return None;
    }
    let name = chamber_display_name(&canonical);
    let config_error = crate::config::load_config(&cryo_toml)
        .err()
        .map(|e| e.to_string());
    let id = encode_id(&canonical);
    let path_hint = if canonical.starts_with(workspace) {
        None
    } else {
        canonical.parent().map(|p| p.display().to_string())
    };
    Some(ChamberEntry {
        id,
        name,
        path: canonical,
        path_hint,
        config_error,
        running: false,
        agent_running: false,
        session: None,
        next_wake: None,
        next_wake_display: None,
        wake_imminent: false,
        has_open_question: false,
        task: None,
        last_message_preview: None,
        completed: false,
        archived: false,
        sync: vec![],
    })
}

fn merge_registry(workspace: &Path, idx: &mut ChamberIndex) {
    let Ok(entries) = crate::registry::list() else {
        return;
    };
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    for registered in entries {
        let archived = registered.archived;
        let path = PathBuf::from(registered.dir);
        let Some(mut entry) = registry_entry_for_path(&workspace, path) else {
            continue;
        };
        entry.archived = archived;
        idx.entry(entry.id.clone()).or_insert(entry);
    }
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
        entry.has_open_question = overview.has_open_question;
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

/// One-shot product discovery: read the registry and populate runtime fields.
pub fn discover(workspace: &Path) -> ChamberIndex {
    discover_with_options(workspace, DiscoveryOptions::all_chambers())
}

pub fn discover_with_options(workspace: &Path, options: DiscoveryOptions) -> ChamberIndex {
    let mut idx = if options.scan_workspace {
        scan_workspace(workspace)
    } else {
        ChamberIndex::new()
    };
    if options.include_registry {
        merge_registry(workspace, &mut idx);
    }
    populate_runtime(&mut idx);
    idx
}

#[cfg(test)]
#[path = "../unit_tests/hub/discovery.rs"]
mod tests;
