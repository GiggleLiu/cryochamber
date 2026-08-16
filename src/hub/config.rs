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
    /// Sender name stamped on messages the owner sends in public mode. The
    /// owner's identity is the server's to decide, not the browser's.
    #[serde(default = "default_owner_name")]
    pub owner_name: String,
    /// Extra `Host` header values the hub accepts, on top of loopback and the
    /// bind host. A reverse proxy that preserves the public hostname (the
    /// documented Caddy deployment does) otherwise trips the DNS-rebinding
    /// guard and every request comes back 403.
    #[serde(default)]
    pub public_hosts: Vec<String>,
    /// Whether bearer auth is enforced. Persisted, so a later plain
    /// `cryohub start` — or a reboot — cannot silently drop the hub back to
    /// open mode while a proxy is still publishing it to the internet.
    #[serde(default)]
    pub public: bool,
    /// Where the built Agent Console lives. Unset means
    /// [`crate::hub::paths::global_console_dir`] (`~/.cryo/console`), which is
    /// where `make console-install` puts it — so the usual install needs no
    /// configuration at all. Set this only to serve a build from somewhere
    /// else, and make it absolute: the hub canonicalizes it from the service
    /// process's working directory, which launchd/systemd choose.
    #[serde(default)]
    pub console_dir: Option<PathBuf>,
}

impl HubConfig {
    /// The directory the console is served from — the configured one, or the
    /// global install path. Always answers; whether a build is actually there
    /// is decided per request, so installing the console does not require
    /// restarting the hub.
    pub fn console_root(&self) -> PathBuf {
        self.console_dir
            .clone()
            .unwrap_or_else(crate::hub::paths::global_console_dir)
    }
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

fn default_owner_name() -> String {
    "human".to_string()
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            chamber_root: default_chamber_root(),
            owner_name: default_owner_name(),
            public_hosts: Vec::new(),
            public: false,
            console_dir: None,
        }
    }
}

/// The configured owner sender name, layered into every request so
/// `post_send` can stamp it without re-reading the config file.
#[derive(Debug, Clone)]
pub struct OwnerName(pub String);

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

/// Load the hub config, apply any CLI overrides, and persist them.
///
/// `public` is `None` when neither `--public` nor `--no-public` was given, in
/// which case the saved mode stands: turning auth *off* has to be an explicit
/// act, never a side effect of restarting without the flag.
pub fn effective_config(
    host: Option<String>,
    port: Option<u16>,
    public: Option<bool>,
) -> Result<HubConfig> {
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
    if let Some(public) = public {
        config.public = public;
        changed = true;
    }
    if changed {
        save_config(&config)?;
    }
    Ok(config)
}
