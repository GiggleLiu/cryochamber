use anyhow::{Context, Result};
use std::path::PathBuf;

/// Return the platform-specific daemon registry directory.
/// Unix: $XDG_RUNTIME_DIR/cryo/ or $HOME/.cryo/daemons/
pub fn registry_dir() -> Result<PathBuf> {
    let dir = if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime).join("cryo")
    } else {
        let home = std::env::var("HOME").context("HOME not set")?;
        PathBuf::from(home).join(".cryo").join("daemons")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
