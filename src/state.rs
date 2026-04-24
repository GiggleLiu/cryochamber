// src/state.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryoState {
    pub session_number: u32,
    pub pid: Option<u32>,
    // --- CLI overrides (only set if user passed explicit flags to `cryo start`) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_session_duration_override: Option<u64>,

    /// Last time a periodic report was sent, stored as an ISO 8601 local time
    /// string without timezone offset (from `Local::now().naive_local()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report_time: Option<String>,

    /// Current provider index for rotation (persisted for status display;
    /// may reflect the last provider used from a previous run until the next
    /// session updates it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_index: Option<usize>,

    /// Identity token for the currently running daemon instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,

    /// True while the daemon has an agent subprocess running a session.
    /// Set `true` before spawning and cleared after `run_one_session` returns
    /// (success or crash), and again on daemon startup. The hub reads this
    /// flag to animate the sidebar "agent running" dot.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub session_active: bool,

    /// True iff the previous session exited without calling `cryo-agent hibernate`.
    /// Used to inject a "previous session crashed" notice into the next prompt so
    /// the agent can check `messages/inbox/archive/` and decide whether any
    /// message still needs a response. Cleared once the notice has been delivered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub previous_session_crashed: bool,
}

pub fn state_path(dir: &Path) -> PathBuf {
    dir.join("timer.json")
}

pub fn save_state(path: &Path, state: &CryoState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let tmp = path.with_file_name(format!(".{file_name}.tmp-{}-{nanos}", std::process::id()));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
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

pub fn new_instance_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}-{:x}", std::process::id(), nanos)
}

pub fn is_locked(state: &CryoState) -> bool {
    if let Some(pid) = state.pid {
        let ret = unsafe { libc::kill(pid as i32, 0) };
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        crate::process::pid_probe_indicates_alive(ret, errno)
    } else {
        false
    }
}

#[cfg(test)]
#[path = "unit_tests/state.rs"]
mod tests;
