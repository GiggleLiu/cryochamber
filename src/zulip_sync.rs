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
            let errno = std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(0);
            errno == libc::EPERM
        }
        None => false,
    }
}

#[cfg(test)]
mod pid_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pid_path_points_into_dir() {
        let p = sync_pid_path(std::path::Path::new("/tmp/cryo-x"));
        assert_eq!(
            p,
            std::path::Path::new("/tmp/cryo-x/cryo-zulip-sync.pid")
        );
    }

    #[test]
    fn read_missing_pid_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_sync_pid(dir.path()).is_none());
    }

    #[test]
    fn read_present_pid_returns_value() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(sync_pid_path(dir.path())).unwrap();
        f.write_all(b"12345\n").unwrap();
        assert_eq!(read_sync_pid(dir.path()), Some(12345));
    }

    #[test]
    fn read_invalid_pid_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(sync_pid_path(dir.path()), "not-a-number").unwrap();
        assert!(read_sync_pid(dir.path()).is_none());
    }

    #[test]
    fn running_is_false_when_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_sync_running(dir.path()));
    }

    #[test]
    fn running_is_false_for_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        let _ = child.wait_with_output();
        std::fs::write(sync_pid_path(dir.path()), dead_pid.to_string()).unwrap();
        assert!(!is_sync_running(dir.path()));
    }
}
