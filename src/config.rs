// src/config.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::state::CryoState;

pub const LEGACY_PROVIDERS_DEPRECATION_WARNING: &str = "Warning: [[providers]] is deprecated; use [provider] instead. Provider rotation has been removed; only one provider is used.";

pub const LEGACY_WATCH_INBOX_DEPRECATION_WARNING: &str =
    "Warning: `watch_inbox` is deprecated; use `watch_dirs = [\"messages/inbox\"]` instead.";

/// Default list of directories the daemon watches for reactive wake.
pub fn default_watch_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from("messages/inbox")]
}

/// A named provider profile with environment variables to inject.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Display name for logging (e.g. "anthropic", "openai")
    pub name: String,
    /// Environment variables to set when spawning the agent
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryoConfig {
    /// Agent command (e.g. "opencode", "claude", "codex")
    #[serde(default = "default_agent")]
    pub agent: String,

    /// Session timeout in seconds (0 = no timeout)
    #[serde(default)]
    pub max_session_duration: u64,

    /// Directories (relative to the chamber root, or absolute) that the
    /// daemon watches for reactive wake. Defaults to just `messages/inbox`.
    #[serde(default = "default_watch_dirs")]
    pub watch_dirs: Vec<PathBuf>,

    /// Legacy boolean form of `watch_dirs`. Accepted on read for backward
    /// compatibility but not written by new configs. `true` maps to the
    /// default watch_dirs, `false` maps to an empty list.
    #[serde(default, skip_serializing)]
    pub watch_inbox: Option<bool>,

    /// Time of day to send periodic report (HH:MM, local time)
    #[serde(default = "default_report_time")]
    pub report_time: String,

    /// Hours between reports (0 = disabled, 24 = daily, 168 = weekly)
    #[serde(default)]
    pub report_interval: u64,

    /// Provider environment profile injected when spawning the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderConfig>,

    /// Legacy provider environment profiles. `[[providers]]` is accepted for
    /// backwards compatibility but is not written by new configs.
    #[serde(default, skip_serializing)]
    pub providers: Vec<ProviderConfig>,

    /// Zulip sync polling interval in seconds (default: 5)
    #[serde(default = "default_poll_interval")]
    pub zulip_poll_interval: u64,

    /// GitHub sync polling interval in seconds (default: 5)
    #[serde(default = "default_poll_interval")]
    pub gh_poll_interval: u64,
}

fn default_agent() -> String {
    "opencode".to_string()
}

fn default_report_time() -> String {
    "09:00".to_string()
}

fn default_poll_interval() -> u64 {
    5
}

impl Default for CryoConfig {
    fn default() -> Self {
        Self {
            agent: default_agent(),
            max_session_duration: 0,
            watch_dirs: default_watch_dirs(),
            watch_inbox: None,
            report_time: default_report_time(),
            report_interval: 0,
            provider: None,
            providers: Vec::new(),
            zulip_poll_interval: default_poll_interval(),
            gh_poll_interval: default_poll_interval(),
        }
    }
}

impl CryoConfig {
    /// Return the single provider profile used for all agent sessions.
    pub fn active_provider(&self) -> Option<&ProviderConfig> {
        self.provider.as_ref().or_else(|| self.providers.first())
    }

    /// True when the config used the deprecated `[[providers]]` array.
    pub fn uses_legacy_providers(&self) -> bool {
        !self.providers.is_empty()
    }

    /// True when the config used the deprecated `watch_inbox` boolean.
    pub fn uses_legacy_watch_inbox(&self) -> bool {
        self.watch_inbox.is_some()
    }

    fn normalize_legacy_provider(&mut self) {
        if self.provider.is_none() {
            self.provider = self.providers.first().cloned();
        }
    }

    /// Translate the legacy `watch_inbox` boolean into `watch_dirs`, unless
    /// the user has already specified `watch_dirs` explicitly. We detect
    /// "user provided watch_dirs" by comparing against the default, since
    /// serde gives us no other signal.
    fn normalize_legacy_watch_inbox(&mut self) {
        let Some(legacy) = self.watch_inbox else {
            return;
        };
        if self.watch_dirs != default_watch_dirs() {
            // Author already migrated; keep their explicit list.
            return;
        }
        self.watch_dirs = if legacy {
            default_watch_dirs()
        } else {
            Vec::new()
        };
    }

    /// Merge CLI overrides from timer.json into this config.
    /// Only overrides fields that were explicitly set (Some).
    pub fn apply_overrides(&mut self, state: &CryoState) {
        apply_optional_override(&mut self.agent, &state.agent_override);
        apply_optional_override(
            &mut self.max_session_duration,
            &state.max_session_duration_override,
        );
    }
}

fn apply_optional_override<T: Clone>(target: &mut T, override_value: &Option<T>) {
    if let Some(value) = override_value {
        *target = value.clone();
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
    let mut config: CryoConfig = toml::from_str(&contents)?;
    if config.uses_legacy_providers() {
        eprintln!("{LEGACY_PROVIDERS_DEPRECATION_WARNING}");
        config.normalize_legacy_provider();
    }
    if config.uses_legacy_watch_inbox() {
        eprintln!("{LEGACY_WATCH_INBOX_DEPRECATION_WARNING}");
        config.normalize_legacy_watch_inbox();
    }
    Ok(Some(config))
}

pub fn save_config(path: &Path, config: &CryoConfig) -> Result<()> {
    let mut config = config.clone();
    config.normalize_legacy_provider();
    config.normalize_legacy_watch_inbox();
    config.watch_inbox = None;
    let toml = toml::to_string_pretty(&config)?;
    std::fs::write(path, toml)?;
    Ok(())
}

#[cfg(test)]
#[path = "unit_tests/config.rs"]
mod tests;
