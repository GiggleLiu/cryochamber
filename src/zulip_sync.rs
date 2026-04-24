use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Persistent state for the Zulip sync utility.
/// Stored in `zulip-sync.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZulipSyncState {
    /// Zulip server URL (e.g. "https://zulip.example.com")
    pub site: String,
    /// Zulip stream name
    pub stream: String,
    /// Zulip stream numeric ID
    pub stream_id: u64,
    /// Bot's email address (used to filter own messages on pull)
    pub self_email: String,
    /// Topic name for outgoing messages (default: "cryochamber")
    #[serde(default)]
    pub topic: Option<String>,
    /// ID of the last fetched message (anchor for polling)
    #[serde(default)]
    pub last_message_id: Option<u64>,
    /// Last session number that was pushed (to prevent duplicate posts)
    #[serde(default)]
    pub last_pushed_session: Option<u32>,
}

impl ZulipSyncState {
    /// Get the topic name, defaulting to "cryochamber".
    pub fn topic_name(&self) -> &str {
        self.topic.as_deref().unwrap_or("cryochamber")
    }
}

pub fn initial_last_message_id(
    import_history: bool,
    newest_message_id: Option<u64>,
) -> Option<u64> {
    if import_history {
        None
    } else {
        newest_message_id
    }
}

pub fn remember_seen_message_id(previous: Option<u64>, seen: Option<u64>) -> Option<u64> {
    match (previous, seen) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (None, Some(b)) => Some(b),
        (Some(a), None) => Some(a),
        (None, None) => None,
    }
}

pub fn save_sync_state(path: &Path, state: &ZulipSyncState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_sync_state(path: &Path) -> Result<Option<ZulipSyncState>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let state: ZulipSyncState = serde_json::from_str(&contents)?;
    Ok(Some(state))
}

pub fn sync_pid_path(dir: &Path) -> PathBuf {
    dir.join("cryo-zulip-sync.pid")
}

pub fn read_sync_pid(dir: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(sync_pid_path(dir)).ok()?;
    content.trim().parse::<u32>().ok()
}

pub fn is_sync_running(dir: &Path) -> bool {
    match read_sync_pid(dir) {
        Some(pid) => {
            let ret = unsafe { libc::kill(pid as i32, 0) };
            if ret == 0 {
                return true;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            errno == libc::EPERM
        }
        None => false,
    }
}

pub fn summarize(dir: &Path) -> Option<crate::sync_common::SyncSummary> {
    let state = load_sync_state(&dir.join("zulip-sync.json"))
        .ok()
        .flatten()?;
    Some(crate::sync_common::SyncSummary {
        backend: crate::sync_common::SyncBackend::Zulip,
        configured: true,
        installed: crate::service::is_installed("zulip-sync", dir),
        running: is_sync_running(dir),
        target: format!("{} · {} / {}", state.site, state.stream, state.topic_name()),
        last_pushed_session: state.last_pushed_session,
        log_tail_path: dir.join("cryo-zulip-sync.log"),
    })
}

#[cfg(test)]
#[path = "unit_tests/zulip_sync.rs"]
mod pid_tests;
