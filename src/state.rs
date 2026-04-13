// src/state.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fallback::FallbackAction;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingFallbackState {
    pub deadline: String,
    pub action: FallbackAction,
}

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

    /// Dead-man switch fallback that should survive daemon restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_fallback: Option<PendingFallbackState>,
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
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("state");
    let tmp = path.with_file_name(format!(
        ".{file_name}.tmp-{}-{nanos}",
        std::process::id()
    ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timer.json");
        std::fs::write(&path, "").unwrap();
        let result = load_state(&path).unwrap();
        assert!(result.is_none(), "Empty file should return None");
    }

    #[test]
    fn test_load_corrupted_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timer.json");
        std::fs::write(&path, "{broken").unwrap();
        let result = load_state(&path);
        assert!(result.is_err(), "Corrupted JSON should return error");
    }

    #[test]
    fn test_load_minimal_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timer.json");
        std::fs::write(&path, r#"{"session_number": 5}"#).unwrap();
        let state = load_state(&path).unwrap().unwrap();
        assert_eq!(state.session_number, 5);
        assert!(state.pid.is_none(), "pid should default to None");
        assert_eq!(state.retry_count, 0, "retry_count should default to 0");
        assert!(state.agent_override.is_none());
    }

    #[test]
    fn test_is_locked_stale_pid() {
        // Spawn a child, wait for it to exit, use its PID
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        // Small delay to ensure the process is fully reaped
        std::thread::sleep(std::time::Duration::from_millis(100));

        let state = CryoState {
            session_number: 1,
            pid: Some(pid),
            retry_count: 0,

            agent_override: None,
            max_retries_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            provider_index: None,
            instance_id: None,
            pending_fallback: None,
        };
        assert!(!is_locked(&state), "Dead PID should not be locked");
    }

    #[test]
    fn test_is_locked_no_pid() {
        let state = CryoState {
            session_number: 1,
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
        assert!(!is_locked(&state), "No PID should not be locked");
    }

    #[test]
    fn test_is_locked_own_pid() {
        let state = CryoState {
            session_number: 1,
            pid: Some(std::process::id()),
            retry_count: 0,

            agent_override: None,
            max_retries_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            provider_index: None,
            instance_id: None,
            pending_fallback: None,
        };
        assert!(is_locked(&state), "Own PID should be locked");
    }
}
