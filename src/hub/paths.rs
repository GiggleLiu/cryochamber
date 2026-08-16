use std::path::{Path, PathBuf};

pub const HUB_LOG_FILENAME: &str = "cryohub.log";

pub fn hub_log_path() -> PathBuf {
    hub_state_dir().join(HUB_LOG_FILENAME)
}

pub fn hub_state_dir() -> PathBuf {
    hub_service_dir_for_state_home(&state_root())
}

pub fn hub_service_dir() -> PathBuf {
    hub_state_dir()
}

pub fn hub_service_dir_for_state_home(state_home: &Path) -> PathBuf {
    state_home.join("cryo").join("hub")
}

pub fn hub_config_path() -> PathBuf {
    config_root().join("cryo").join("cryohub.toml")
}

pub fn hub_tokens_path() -> PathBuf {
    config_root().join("cryo").join("cryohub-tokens.json")
}

pub fn global_chambers_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".cryo").join("chambers");
    }

    std::env::temp_dir().join(".cryo").join("chambers")
}

/// Where `make console-install` puts the built Agent Console, and where the
/// hub looks when `console_dir` is not configured. A machine-global path
/// beside the chamber registry, deliberately not inside any checkout: the hub
/// runs as a service and serves this directory for as long as it lives.
pub fn global_console_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".cryo").join("console");
    }

    std::env::temp_dir().join(".cryo").join("console")
}

fn state_root() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_STATE_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }

    if let Some(home) = dirs::home_dir() {
        #[cfg(target_os = "macos")]
        {
            return home.join("Library").join("Logs");
        }

        #[cfg(not(target_os = "macos"))]
        {
            return home.join(".local").join("state");
        }
    }

    std::env::temp_dir()
}

fn config_root() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }

    if let Some(home) = dirs::home_dir() {
        return home.join(".config");
    }

    std::env::temp_dir()
}
