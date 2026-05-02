// src/registry.rs
//! User registry for tracking cryochamber directories and their daemon PIDs.
//!
//! Each chamber gets a persistent entry in the user state directory. Daemon
//! startup fills in `pid`/`socket_path`; clean shutdown clears those runtime
//! fields but keeps the chamber entry so Cryohub can still surface stopped
//! chambers. Stale PIDs and entries whose chamber directory disappeared are
//! repaired when the registry is read.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonEntry {
    #[serde(default)]
    pub pid: Option<u32>,
    pub dir: String,
    #[serde(default)]
    pub socket_path: Option<String>,
}

/// Return the registry directory, creating it if needed.
///
/// Prefers `$XDG_STATE_HOME/cryo/chambers/` so stopped chambers survive
/// daemon exit and reboot, falls back to `~/.cryo/chambers/`.
fn registry_dir() -> Result<PathBuf> {
    let dir = if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
        PathBuf::from(state_home).join("cryo").join("chambers")
    } else {
        let home = std::env::var("HOME").context("HOME not set")?;
        PathBuf::from(home).join(".cryo").join("chambers")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Stable filename for a given working directory.
fn entry_filename(dir: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut hasher);
    format!("{:016x}.json", hasher.finish())
}

/// Remember a chamber in the user registry without marking it running.
pub fn remember_chamber(dir: &Path) -> Result<()> {
    write_entry(
        dir,
        DaemonEntry {
            pid: None,
            dir: dir.to_string_lossy().to_string(),
            socket_path: None,
        },
    )
}

/// Register this daemon in the user registry.
pub fn register(dir: &Path, socket_path: Option<&Path>) -> Result<()> {
    write_entry(
        dir,
        DaemonEntry {
            pid: Some(std::process::id()),
            dir: dir.to_string_lossy().to_string(),
            socket_path: socket_path.map(|p| p.to_string_lossy().to_string()),
        },
    )
}

fn write_entry(dir: &Path, entry: DaemonEntry) -> Result<()> {
    let reg = registry_dir()?;
    let path = reg.join(entry_filename(dir));
    std::fs::write(&path, serde_json::to_string(&entry)?)?;
    Ok(())
}

/// Mark this daemon as stopped while preserving the chamber entry.
pub fn unregister(dir: &Path) {
    let entry = DaemonEntry {
        pid: None,
        dir: dir.to_string_lossy().to_string(),
        socket_path: None,
    };
    let _ = write_entry(dir, entry);
}

/// List all remembered chambers. Missing chamber directories are pruned;
/// dead PIDs are cleared so the entry remains visible as stopped.
pub fn list() -> Result<Vec<DaemonEntry>> {
    let reg = registry_dir()?;
    let mut entries = Vec::new();

    let dir = match std::fs::read_dir(&reg) {
        Ok(d) => d,
        Err(_) => return Ok(entries),
    };

    for file in dir {
        let file = file?;
        if file.path().extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let content = match std::fs::read_to_string(file.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut entry: DaemonEntry = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(_) => {
                let _ = std::fs::remove_file(file.path());
                continue;
            }
        };

        let chamber_dir = PathBuf::from(&entry.dir);
        if !crate::config::config_path(&chamber_dir).exists() {
            let _ = std::fs::remove_file(file.path());
            continue;
        }

        if let Some(pid) = entry.pid {
            if !is_pid_alive(pid) {
                entry.pid = None;
                entry.socket_path = None;
                let _ = std::fs::write(file.path(), serde_json::to_string(&entry)?);
            }
        }

        entries.push(entry);
    }

    Ok(entries)
}

fn is_pid_alive(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as i32, 0) };
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    crate::process::pid_probe_indicates_alive(ret, errno)
}

#[cfg(test)]
#[path = "unit_tests/registry.rs"]
mod tests;
