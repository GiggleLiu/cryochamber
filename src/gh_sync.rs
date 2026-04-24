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

    pub fn ensure_self_login_with<F>(&mut self, lookup: F) -> Result<bool>
    where
        F: FnOnce() -> Result<String>,
    {
        if self.self_login.is_some() {
            return Ok(false);
        }

        self.self_login = Some(lookup()?);
        Ok(true)
    }

    pub fn status_lines(&self) -> Vec<String> {
        vec![
            format!("Repo: {}", self.repo),
            format!("Discussion: #{}", self.discussion_number),
            format!(
                "GitHub user: {}",
                self.self_login.as_deref().unwrap_or("(unknown)")
            ),
            format!(
                "Last read position: {}",
                self.last_read_cursor
                    .as_deref()
                    .unwrap_or("(none - will read all)")
            ),
            format!(
                "Last pushed session: {}",
                self.last_pushed_session
                    .map(|session| session.to_string())
                    .unwrap_or_else(|| "(none)".to_string())
            ),
        ]
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
#[path = "unit_tests/gh_sync.rs"]
mod pid_tests;
