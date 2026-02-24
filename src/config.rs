// src/config.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::state::CryoState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryoConfig {
    /// Agent command (e.g. "opencode", "claude", "codex")
    #[serde(default = "default_agent")]
    pub agent: String,

    /// Path to the plan file
    #[serde(default = "default_plan_path")]
    pub plan_path: String,

    /// Max retry attempts on agent failure (1 = no retry)
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Session timeout in seconds (0 = no timeout)
    #[serde(default)]
    pub max_session_duration: u64,

    /// Watch inbox for reactive wake
    #[serde(default = "default_watch_inbox")]
    pub watch_inbox: bool,
}

fn default_agent() -> String {
    "opencode".to_string()
}

fn default_plan_path() -> String {
    "plan.md".to_string()
}

fn default_max_retries() -> u32 {
    1
}

fn default_watch_inbox() -> bool {
    true
}

impl Default for CryoConfig {
    fn default() -> Self {
        Self {
            agent: default_agent(),
            plan_path: default_plan_path(),
            max_retries: default_max_retries(),
            max_session_duration: 0,
            watch_inbox: default_watch_inbox(),
        }
    }
}

impl CryoConfig {
    /// Merge CLI overrides from timer.json into this config.
    /// Only overrides fields that were explicitly set (Some).
    pub fn apply_overrides(&mut self, state: &CryoState) {
        if let Some(ref agent) = state.agent_override {
            self.agent = agent.clone();
        }
        if let Some(max_retries) = state.max_retries_override {
            self.max_retries = max_retries;
        }
        if let Some(max_session_duration) = state.max_session_duration_override {
            self.max_session_duration = max_session_duration;
        }
    }
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("cryo.toml")
}

pub fn load_config(path: &Path) -> Result<Option<CryoConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let config: CryoConfig = toml::from_str(&contents)?;
    Ok(Some(config))
}

pub fn save_config(path: &Path, config: &CryoConfig) -> Result<()> {
    let toml = toml::to_string_pretty(config)?;
    std::fs::write(path, toml)?;
    Ok(())
}
