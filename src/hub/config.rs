use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_chamber_root")]
    pub chamber_root: PathBuf,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8765
}

fn default_chamber_root() -> PathBuf {
    crate::hub::paths::global_chambers_dir()
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            chamber_root: default_chamber_root(),
        }
    }
}

pub fn load_config() -> Result<HubConfig> {
    let path = crate::hub::paths::hub_config_path();
    if !path.exists() {
        return Ok(HubConfig::default());
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(toml::from_str(&text)?)
}

pub fn save_config(config: &HubConfig) -> Result<()> {
    let path = crate::hub::paths::hub_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

pub fn load_or_create_config() -> Result<HubConfig> {
    let config = load_config()?;
    if !crate::hub::paths::hub_config_path().exists() {
        save_config(&config)?;
    }
    Ok(config)
}

pub fn effective_config(host: Option<String>, port: Option<u16>) -> Result<HubConfig> {
    let mut config = load_or_create_config()?;
    let mut changed = false;
    if let Some(host) = host {
        config.host = host;
        changed = true;
    }
    if let Some(port) = port {
        config.port = port;
        changed = true;
    }
    if changed {
        save_config(&config)?;
    }
    Ok(config)
}
