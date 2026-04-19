use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Persistent state for the GitHub Discussion sync utility.
/// Stored in `gh-sync.json`, separate from `timer.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhSyncState {
    /// GitHub repo in "owner/repo" format
    pub repo: String,
    /// GitHub Discussion number
    pub discussion_number: u64,
    /// GitHub Discussion node ID (for GraphQL mutations)
    pub discussion_node_id: String,
    /// Pagination cursor for fetching new Discussion comments
    #[serde(default)]
    pub last_read_cursor: Option<String>,
    /// Login of the authenticated GitHub user (used to filter own comments on pull)
    #[serde(default)]
    pub self_login: Option<String>,
    /// Last session number that was pushed (to prevent duplicate posts)
    #[serde(default)]
    pub last_pushed_session: Option<u32>,
}

impl GhSyncState {
    /// Split repo into (owner, repo_name).
    pub fn owner_repo(&self) -> Result<(&str, &str)> {
        self.repo
            .split_once('/')
            .context("repo must be in 'owner/repo' format")
    }
}

pub fn save_sync_state(path: &Path, state: &GhSyncState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_sync_state(path: &Path) -> Result<Option<GhSyncState>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let state: GhSyncState = serde_json::from_str(&contents)?;
    Ok(Some(state))
}

pub fn sync_pid_path(dir: &Path) -> PathBuf {
    dir.join("cryo-gh-sync.pid")
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
    let state = load_sync_state(&dir.join("gh-sync.json")).ok().flatten()?;
    Some(crate::sync_common::SyncSummary {
        backend: crate::sync_common::SyncBackend::Gh,
        configured: true,
        installed: crate::service::is_installed("gh-sync", dir),
        running: is_sync_running(dir),
        target: format!("{}#{}", state.repo, state.discussion_number),
        last_pushed_session: state.last_pushed_session,
        log_tail_path: dir.join("cryo-gh-sync.log"),
    })
}

#[cfg(test)]
mod pid_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pid_path_points_into_dir() {
        let p = sync_pid_path(std::path::Path::new("/tmp/cryo-x"));
        assert_eq!(p, std::path::Path::new("/tmp/cryo-x/cryo-gh-sync.pid"));
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
