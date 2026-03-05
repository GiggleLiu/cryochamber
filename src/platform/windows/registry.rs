use anyhow::{Context, Result};
use std::path::PathBuf;

/// Return the platform-specific daemon registry directory.
/// Windows: %LOCALAPPDATA%\cryo\daemons\
pub fn registry_dir() -> Result<PathBuf> {
    let dir = dirs::data_local_dir()
        .context("Failed to determine LOCALAPPDATA")?
        .join("cryo")
        .join("daemons");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_dir_exists() {
        let dir = registry_dir().unwrap();
        assert!(dir.exists());
        assert!(dir.to_string_lossy().contains("cryo"));
    }
}
