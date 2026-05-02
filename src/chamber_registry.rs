//! Durable user-level registry of known chamber directories.
//!
//! This is separate from `crate::registry`, which tracks live daemon PIDs.
//! The durable registry stores chamber paths so `cryohub` can show stopped
//! chambers that were previously started elsewhere on the same user account.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const REGISTRY_ENV: &str = "CRYO_CHAMBER_REGISTRY";

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    chambers: Vec<PathBuf>,
}

/// Return the durable chamber registry path.
pub fn registry_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var(REGISTRY_ENV) {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".cryo").join("chambers.json"))
}

/// Record a chamber in the default durable registry.
pub fn record(dir: &Path) -> Result<()> {
    record_at(&registry_path()?, dir)
}

/// Record a chamber in a specific registry file.
pub fn record_at(registry_path: &Path, dir: &Path) -> Result<()> {
    let mut registry = load_file(registry_path)?;
    let path = canonical_or_original(dir);
    if !registry.chambers.iter().any(|existing| existing == &path) {
        registry.chambers.push(path);
        registry.chambers.sort();
    }
    save_file(registry_path, &registry)
}

/// List raw registry entries without pruning.
pub fn list_at(registry_path: &Path) -> Result<Vec<PathBuf>> {
    Ok(load_file(registry_path)?.chambers)
}

/// Import running daemon entries into a specific durable registry file.
///
/// Only paths that still look like chambers are recorded.
pub fn import_daemon_entries_at(
    registry_path: &Path,
    entries: &[crate::registry::DaemonEntry],
) -> Result<()> {
    for entry in entries {
        let dir = PathBuf::from(&entry.dir);
        if is_chamber_dir(&dir) {
            record_at(registry_path, &dir)?;
        }
    }
    Ok(())
}

/// Import currently-running daemons into the default durable registry.
pub fn import_running_daemons() -> Result<()> {
    let entries = crate::registry::list()?;
    import_daemon_entries_at(&registry_path()?, &entries)
}

/// Prune invalid entries from the default durable registry.
pub fn prune_invalid() -> Result<Vec<PathBuf>> {
    prune_invalid_at(&registry_path()?)
}

/// Remove missing and non-chamber paths from a registry file.
///
/// A chamber path is valid if the directory exists and contains `cryo.toml`;
/// config parse errors are left for the hub to surface in the UI.
pub fn prune_invalid_at(registry_path: &Path) -> Result<Vec<PathBuf>> {
    let registry = load_file(registry_path)?;
    let mut valid = Vec::new();
    for path in registry.chambers {
        if is_chamber_dir(&path) {
            let canonical = canonical_or_original(&path);
            if !valid.iter().any(|existing| existing == &canonical) {
                valid.push(canonical);
            }
        }
    }
    valid.sort();
    save_file(
        registry_path,
        &RegistryFile {
            chambers: valid.clone(),
        },
    )?;
    Ok(valid)
}

fn load_file(path: &Path) -> Result<RegistryFile> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RegistryFile::default()),
        Err(e) => Err(e).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn save_file(path: &Path, registry: &RegistryFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(registry)?;
    std::fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_chamber_dir(path: &Path) -> bool {
    path.is_dir() && crate::config::config_path(path).exists()
}

#[cfg(test)]
#[path = "unit_tests/chamber_registry.rs"]
mod tests;
