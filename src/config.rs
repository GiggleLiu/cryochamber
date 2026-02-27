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

    /// Max retry attempts on agent failure (0 = no retry)
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Session timeout in seconds (0 = no timeout)
    #[serde(default)]
    pub max_session_duration: u64,

    /// Watch inbox for reactive wake
    #[serde(default = "default_watch_inbox")]
    pub watch_inbox: bool,

    /// Web UI host (default: 127.0.0.1)
    #[serde(default = "default_web_host")]
    pub web_host: String,

    /// Web UI port (default: 3945)
    #[serde(default = "default_web_port")]
    pub web_port: u16,

    /// Fallback alert method: "notify" (desktop popup), "outbox" (file only), "none"
    #[serde(default = "default_fallback_alert")]
    pub fallback_alert: String,

    /// Time of day to send periodic report (HH:MM, local time)
    #[serde(default = "default_report_time")]
    pub report_time: String,

    /// Hours between reports (0 = disabled, 24 = daily, 168 = weekly)
    #[serde(default)]
    pub report_interval: u64,
}

fn default_agent() -> String {
    "opencode".to_string()
}

fn default_max_retries() -> u32 {
    5
}

fn default_watch_inbox() -> bool {
    true
}

fn default_web_host() -> String {
    "127.0.0.1".to_string()
}

fn default_web_port() -> u16 {
    3945
}

fn default_fallback_alert() -> String {
    "notify".to_string()
}

fn default_report_time() -> String {
    "09:00".to_string()
}

impl Default for CryoConfig {
    fn default() -> Self {
        Self {
            agent: default_agent(),
            max_retries: default_max_retries(),
            max_session_duration: 0,
            watch_inbox: default_watch_inbox(),
            web_host: default_web_host(),
            web_port: default_web_port(),
            fallback_alert: default_fallback_alert(),
            report_time: default_report_time(),
            report_interval: 24,
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
