use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
