// src/config.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::state::CryoState;

pub const LEGACY_PROVIDERS_DEPRECATION_WARNING: &str = "Warning: [[providers]] is deprecated; use [provider] instead. Provider rotation has been removed; only one provider is used.";
pub const DEFAULT_MAX_SESSION_DURATION_SECS: u64 = 3600;
/// Reply window applied when `reply_window` is absent from
/// `cryo.toml`. An explicit `reply_window = 0` disables the window.
pub const DEFAULT_REPLY_WINDOW_SECS: u64 = 300;
/// Upper bound the daemon applies to `reply_window`, however it is configured.
pub const MAX_REPLY_WINDOW_SECS: u64 = 86400;

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
    /// Agent command (e.g. "opencode", "claude", "codex", "pi", "kimi")
    #[serde(default = "default_agent")]
    pub agent: String,

    /// Session timeout in seconds (0 = no timeout)
    #[serde(default = "default_max_session_duration")]
    pub max_session_duration: u64,

    /// Reply window in seconds kept open after the agent hibernates: a
    /// message arriving inside the window is handled by the same session.
    /// `None` = `DEFAULT_REPLY_WINDOW_SECS` (300); an explicit 0 disables
    /// the window; the daemon caps any value at 86400.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_window: Option<u64>,

    /// Directories (relative to the chamber root, or absolute) that the
    /// daemon watches for reactive wake. Defaults to just `messages/inbox`.
    #[serde(default = "default_watch_dirs")]
    pub watch_dirs: Vec<PathBuf>,

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
}

fn default_agent() -> String {
    "opencode".to_string()
}

fn default_max_session_duration() -> u64 {
    DEFAULT_MAX_SESSION_DURATION_SECS
}

fn default_poll_interval() -> u64 {
    5
}

impl Default for CryoConfig {
    fn default() -> Self {
        Self {
            agent: default_agent(),
            max_session_duration: default_max_session_duration(),
            reply_window: None,
            watch_dirs: default_watch_dirs(),
            provider: None,
            providers: Vec::new(),
            zulip_poll_interval: default_poll_interval(),
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

    fn normalize_legacy_provider(&mut self) {
        if self.provider.is_none() {
            self.provider = self.providers.first().cloned();
        }
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
    Ok(Some(config))
}

pub fn save_config(path: &Path, config: &CryoConfig) -> Result<()> {
    let mut config = config.clone();
    config.normalize_legacy_provider();
    let toml = toml::to_string_pretty(&config)?;
    std::fs::write(path, toml)?;
    // cryo.toml may hold a provider API key in `[provider].env`, so keep it
    // owner-readable only. Applied to every writer since any of them can carry
    // secrets.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "unit_tests/config.rs"]
mod tests;
