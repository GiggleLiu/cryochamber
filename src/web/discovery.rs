//! Chamber discovery: scan `./chambers/*/cryo.toml` and merge with the daemon registry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where a chamber was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Under `./chambers/` in the workspace.
    Workspace,
    /// Running daemon registered elsewhere on the machine.
    External,
}

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
    pub source: Source,
    pub config_error: Option<String>,
    pub running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub unread: usize,
    pub completed: bool,
    pub sync: Vec<SyncBadge>,
}

/// A map from chamber id → entry.
pub type ChamberIndex = BTreeMap<String, ChamberEntry>;

/// Scan `<workspace>/chambers/*` for chambers. Returns entries for every
/// subdirectory (even ones with broken or missing `cryo.toml` — those get a
/// `config_error`). Runtime fields (`running`, `session`, `next_wake`,
/// `unread`) are filled in by `populate_runtime`, not here.
pub fn scan_workspace(workspace: &Path) -> ChamberIndex {
    let chambers_dir = workspace.join("chambers");
    let mut out = ChamberIndex::new();
    let Ok(rd) = std::fs::read_dir(&chambers_dir) else {
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
                source: Source::Workspace,
                config_error,
                running: false,
                session: None,
                next_wake: None,
                unread: 0,
                completed: false,
                sync: vec![],
            },
        );
    }
    out
}

/// Merge running daemons from `entries` into `idx`. Entries whose path is
/// already present in the index (keyed by canonicalized path) simply flip
/// `running = true`; entries whose path is new get added with
/// `source = External`.
pub fn merge_registry(idx: &mut ChamberIndex, entries: &[crate::registry::DaemonEntry]) {
    for entry in entries {
        let raw = PathBuf::from(&entry.dir);
        let canonical = raw.canonicalize().unwrap_or(raw);
        let id = encode_id(&canonical);
        if let Some(existing) = idx.get_mut(&id) {
            existing.running = true;
            continue;
        }
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(unknown)".into());
        idx.insert(
            id.clone(),
            ChamberEntry {
                id,
                name,
                path: canonical,
                source: Source::External,
                config_error: None,
                running: true,
                session: None,
                next_wake: None,
                unread: 0,
                completed: false,
                sync: vec![],
            },
        );
    }
}

/// Fill in runtime fields on each entry from its on-disk state.
/// `running` is left as-is if already true (set by `merge_registry`).
pub fn populate_runtime(idx: &mut ChamberIndex) {
    for entry in idx.values_mut() {
        let dir = &entry.path;

        // Session # and pid from timer.json
        if let Ok(Some(st)) = crate::state::load_state(&crate::state::state_path(dir)) {
            entry.session = Some(st.session_number);
            if !entry.running {
                entry.running = crate::state::is_locked(&st);
            }
        }

        // Next wake from todo.json
        let todo_path = dir.join("todo.json");
        entry.next_wake = crate::todo::TodoList::load(&todo_path)
            .ok()
            .and_then(|list| list.next_wake_time().map(String::from));

        // Unread = pending inbox messages (not archived)
        entry.unread = crate::message::read_inbox(dir)
            .map(|v| v.len())
            .unwrap_or(0);

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

/// One-shot discovery: scan workspace, merge registry, populate runtime.
pub fn discover(workspace: &Path) -> ChamberIndex {
    let mut idx = scan_workspace(workspace);
    if let Ok(entries) = crate::registry::list() {
        merge_registry(&mut idx, &entries);
    }
    populate_runtime(&mut idx);
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let path = PathBuf::from("/Users/alice/work space/chambers/my chamber");
        let id = encode_id(&path);
        assert!(!id.contains(' '), "id must be URL-safe");
        assert!(!id.contains('/'), "id must not contain raw slashes");
        let back = decode_id(&id).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn decode_rejects_invalid() {
        // %ZZ is not valid percent-encoding
        assert!(decode_id("%ZZ").is_none());
    }

    #[test]
    fn source_serialises_lowercase() {
        let json = serde_json::to_string(&Source::Workspace).unwrap();
        assert_eq!(json, "\"workspace\"");
        let json = serde_json::to_string(&Source::External).unwrap();
        assert_eq!(json, "\"external\"");
    }

    #[test]
    fn scan_empty_workspace_returns_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let idx = scan_workspace(dir.path());
        assert!(idx.is_empty());
    }

    #[test]
    fn scan_finds_chambers_with_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(chambers.join("alpha")).unwrap();
        std::fs::create_dir_all(chambers.join("beta")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&chambers.join("alpha").join("cryo.toml"), &cfg).unwrap();
        crate::config::save_config(&chambers.join("beta").join("cryo.toml"), &cfg).unwrap();

        let idx = scan_workspace(dir.path());
        assert_eq!(idx.len(), 2);
        let names: Vec<_> = idx.values().map(|e| e.name.clone()).collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
        for entry in idx.values() {
            assert_eq!(entry.source, Source::Workspace);
            assert!(entry.config_error.is_none());
        }
    }

    #[test]
    fn scan_flags_missing_cryo_toml_as_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("chambers").join("broken")).unwrap();
        let idx = scan_workspace(dir.path());
        assert_eq!(idx.len(), 1);
        let entry = idx.values().next().unwrap();
        assert!(entry.config_error.is_some());
    }

    #[test]
    fn external_daemon_appears_with_external_source() {
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("somewhere-else");
        std::fs::create_dir_all(&external).unwrap();
        let mut idx = ChamberIndex::new();
        merge_registry(
            &mut idx,
            &[crate::registry::DaemonEntry {
                pid: 1,
                dir: external.to_string_lossy().into_owned(),
                socket_path: None,
            }],
        );
        assert_eq!(idx.len(), 1);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.source, Source::External);
        assert!(entry.running);
    }

    #[test]
    fn running_workspace_chamber_flips_running_not_source() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(chambers.join("alpha")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&chambers.join("alpha").join("cryo.toml"), &cfg).unwrap();

        let mut idx = scan_workspace(dir.path());
        let alpha_path = chambers.join("alpha").canonicalize().unwrap();
        merge_registry(
            &mut idx,
            &[crate::registry::DaemonEntry {
                pid: 42,
                dir: alpha_path.to_string_lossy().into_owned(),
                socket_path: None,
            }],
        );
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.source, Source::Workspace);
        assert!(entry.running);
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_chamber_is_deduped() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-chamber");
        std::fs::create_dir_all(&real).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&real.join("cryo.toml"), &cfg).unwrap();

        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(&chambers).unwrap();
        std::os::unix::fs::symlink(&real, chambers.join("alpha")).unwrap();

        let mut idx = scan_workspace(dir.path());
        let real_canonical = real.canonicalize().unwrap();
        merge_registry(
            &mut idx,
            &[crate::registry::DaemonEntry {
                pid: 1,
                dir: real_canonical.to_string_lossy().into_owned(),
                socket_path: None,
            }],
        );
        assert_eq!(idx.len(), 1);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.source, Source::Workspace);
        assert!(entry.running);
    }

    #[test]
    fn populate_reads_session_and_unread() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

        // Fake runtime state: session 7, not locked (no live PID)
        let st = crate::state::CryoState {
            session_number: 7,
            pid: None,
            retry_count: 0,
            agent_override: None,
            max_retries_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            provider_index: None,
            instance_id: None,
            pending_fallback: None,
        };
        crate::state::save_state(&crate::state::state_path(&alpha), &st).unwrap();

        // Fake inbox with one message
        crate::message::ensure_dirs(&alpha).unwrap();
        let msg = crate::message::Message {
            from: "tester".into(),
            subject: "hi".into(),
            body: "yo".into(),
            timestamp: chrono::Local::now().naive_local(),
            metadata: Default::default(),
        };
        crate::message::write_message(&alpha, "inbox", &msg).unwrap();

        let mut idx = scan_workspace(dir.path());
        populate_runtime(&mut idx);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.session, Some(7));
        assert_eq!(entry.unread, 1);
        assert!(!entry.running, "no live pid -> not running");
    }

    #[test]
    fn populate_reports_configured_gh_sync() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let state = crate::gh_sync::GhSyncState {
            repo: "a/b".into(),
            discussion_number: 1,
            discussion_node_id: "n".into(),
            last_read_cursor: None,
            self_login: None,
            last_pushed_session: None,
        };
        crate::gh_sync::save_sync_state(&alpha.join("gh-sync.json"), &state).unwrap();

        let mut idx = scan_workspace(dir.path());
        populate_runtime(&mut idx);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.sync.len(), 1);
        assert_eq!(entry.sync[0].backend, "gh");
        assert!(!entry.sync[0].running);
    }
}
