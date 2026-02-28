// src/state.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryoState {
    pub session_number: u32,
    pub pid: Option<u32>,
    /// Current retry count for the active wake cycle. Reset to 0 on success.
    #[serde(default)]
    pub retry_count: u32,
    // --- CLI overrides (only set if user passed explicit flags to `cryo start`) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_override: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_session_duration_override: Option<u64>,
    /// Scheduled next wake time (ISO 8601 format), set by daemon on hibernate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_wake: Option<String>,

    /// Last time a periodic report was sent, stored as an ISO 8601 local time
    /// string without timezone offset (from `Local::now().naive_local()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report_time: Option<String>,

    /// Current provider index for rotation (persisted for status display)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_index: Option<usize>,
}

pub fn state_path(dir: &Path) -> PathBuf {
    dir.join("timer.json")
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
    if contents.trim().is_empty() {
        // File exists but is empty — likely caught mid-write (truncate-then-write race).
        return Ok(None);
    }
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
