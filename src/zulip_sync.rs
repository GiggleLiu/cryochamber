use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
