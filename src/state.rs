// src/state.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryoState {
    pub plan_path: String,
    pub session_number: u32,
    pub last_command: Option<String>,
    pub wake_timer_id: Option<String>,
    pub fallback_timer_id: Option<String>,
    pub pid: Option<u32>,
    /// Maximum number of retry attempts on agent spawn failure.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Current retry count for the active wake cycle. Reset to 0 on success.
    #[serde(default)]
    pub retry_count: u32,
    /// Maximum session duration in seconds. 0 = no timeout.
    #[serde(default = "default_max_session_duration")]
    pub max_session_duration: u64,
    /// Whether the daemon should watch messages/inbox/ for reactive wake.
    #[serde(default = "default_watch_inbox")]
    pub watch_inbox: bool,
    /// Whether this instance is running as a daemon (vs one-shot wake).
    #[serde(default)]
    pub daemon_mode: bool,
}

fn default_max_retries() -> u32 {
    1
}

fn default_max_session_duration() -> u64 {
    1800 // 30 minutes
}

fn default_watch_inbox() -> bool {
    true
}

pub fn save_state(path: &Path, state: &CryoState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_state(path: &Path) -> Result<Option<CryoState>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let state: CryoState = serde_json::from_str(&contents)?;
    Ok(Some(state))
}

pub fn is_locked(state: &CryoState) -> bool {
    if let Some(pid) = state.pid {
        let ret = unsafe { libc::kill(pid as i32, 0) };
        if ret == 0 {
            return true;
        }
        // EPERM means process exists but we lack permission — still locked
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        errno == libc::EPERM
    } else {
        false
    }
}
