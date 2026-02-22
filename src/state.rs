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
        unsafe { libc::kill(pid as i32, 0) == 0 }
    } else {
        false
    }
}
