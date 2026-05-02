use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub const HUB_LOG_FILENAME: &str = "cryohub.log";

pub fn hub_log_path(workspace_dir: &Path) -> PathBuf {
    hub_state_dir(workspace_dir).join(HUB_LOG_FILENAME)
}

pub fn hub_state_dir(workspace_dir: &Path) -> PathBuf {
    state_root()
        .join("cryo")
        .join("hubs")
        .join(path_hash(workspace_dir))
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

fn path_hash(path: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
