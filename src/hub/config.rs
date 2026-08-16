use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted hub configuration (`cryohub.toml`).
///
/// Unknown keys are an error, not a warning: a misspelled key that loads
/// silently is worse than one that refuses, because the next `save_config`
/// would erase it and the operator would never learn why the setting had no
/// effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Overrides the console embedded in the binary. Set this only to serve a
    /// build from somewhere else, and make it absolute: the hub canonicalizes
    /// it from the service process's working directory, which launchd/systemd
    /// choose.
    #[serde(default)]
    pub console_dir: Option<PathBuf>,
}

impl HubConfig {
    /// Where the console is served from: an operator override, else the build
    /// embedded in the binary. Whether a build is actually there is decided
    /// per request, so an override directory may be filled without restarting.
    pub fn console_source(&self) -> crate::hub::routes::console::ConsoleSource {
        match &self.console_dir {
            Some(dir) => crate::hub::routes::console::ConsoleSource::Dir(dir.clone()),
            None => crate::hub::routes::console::ConsoleSource::Embedded,
        }
    }

    /// A relative `console_dir` would be resolved from the service process's
    /// working directory, which launchd/systemd choose — so it is refused
    /// before a service is installed rather than 503ing after a reboot.
    pub fn validate_console_dir(&self) -> anyhow::Result<()> {
        if let Some(dir) = &self.console_dir {
            anyhow::ensure!(
                dir.is_absolute(),
                "console_dir in {} must be an absolute path (got {})",
                crate::hub::paths::hub_config_path().display(),
                dir.display()
            );
        }
        Ok(())
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
    // Name the file in every failure: a rejected unknown key says which key,
    // but `cryohub.toml` is not somewhere an operator would think to look
    // without being told where it is.
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
}

/// Write the config atomically: the whole file lands under a temporary name
/// and is renamed into place, so a crash mid-write (or a concurrent reader)
/// never sees a truncated `cryohub.toml`.
pub fn save_config(config: &HubConfig) -> Result<()> {
    let path = crate::hub::paths::hub_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(config)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn load_or_create_config() -> Result<HubConfig> {
    let config = load_config()?;
    if !crate::hub::paths::hub_config_path().exists() {
        save_config(&config)?;
    }
    Ok(config)
}

/// Apply CLI overrides to a loaded config, in memory only.
///
/// `public` is `None` when neither `--public` nor `--no-public` was given, in
/// which case the saved mode stands: turning auth *off* has to be an explicit
/// act, never a side effect of restarting without the flag.
pub fn overlay_config(
    mut config: HubConfig,
    host: Option<String>,
    port: Option<u16>,
    public: Option<bool>,
) -> HubConfig {
    if let Some(host) = host {
        config.host = host;
    }
    if let Some(port) = port {
        config.port = port;
    }
    if let Some(public) = public {
        config.public = public;
    }
    config
}

/// Load the hub config, apply any CLI overrides, and persist them. This is the
/// `cryohub start` path — the one place flags become configuration. The
/// service unit's `cryohub daemon` uses [`overlay_config`] without saving.
pub fn effective_config(
    host: Option<String>,
    port: Option<u16>,
    public: Option<bool>,
) -> Result<HubConfig> {
    let base = load_or_create_config()?;
    let config = overlay_config(base.clone(), host, port, public);
    if config != base {
        save_config(&config)?;
    }
    Ok(config)
}
