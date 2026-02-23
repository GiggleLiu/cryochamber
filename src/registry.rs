// src/registry.rs
//! PID file registry for tracking running cryo daemons.
//!
//! Each daemon registers itself in `$XDG_RUNTIME_DIR/cryo/` (or `~/.cryo/daemons/`)
//! on startup and removes the file on clean exit. `cryo ps` reads the directory
//! to list all known daemons. Stale entries (dead PIDs) are auto-cleaned on read.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonEntry {
    pub pid: u32,
    pub dir: String,
}

/// Return the registry directory, creating it if needed.
///
/// Prefers `$XDG_RUNTIME_DIR/cryo/` (auto-cleaned on reboot by the OS),
/// falls back to `~/.cryo/daemons/`.
fn registry_dir() -> Result<PathBuf> {
    let dir = if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime).join("cryo")
    } else {
        let home = std::env::var("HOME").context("HOME not set")?;
        PathBuf::from(home).join(".cryo").join("daemons")
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

/// Register this daemon in the global registry.
pub fn register(dir: &Path) -> Result<()> {
    let reg = registry_dir()?;
    let entry = DaemonEntry {
        pid: std::process::id(),
        dir: dir.to_string_lossy().to_string(),
    };
    let path = reg.join(entry_filename(dir));
    std::fs::write(&path, serde_json::to_string(&entry)?)?;
    Ok(())
}

/// Remove this daemon from the global registry.
pub fn unregister(dir: &Path) {
    if let Ok(reg) = registry_dir() {
        let path = reg.join(entry_filename(dir));
        let _ = std::fs::remove_file(path);
    }
}

/// List all registered daemons. Dead entries are auto-cleaned.
pub fn list() -> Result<Vec<DaemonEntry>> {
    let reg = registry_dir()?;
    let mut alive = Vec::new();

    let dir = match std::fs::read_dir(&reg) {
        Ok(d) => d,
        Err(_) => return Ok(alive),
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
        let entry: DaemonEntry = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(_) => {
                let _ = std::fs::remove_file(file.path());
                continue;
            }
        };

        if is_pid_alive(entry.pid) {
            alive.push(entry);
        } else {
            // Auto-clean stale entry
            let _ = std::fs::remove_file(file.path());
        }
    }

    Ok(alive)
}

fn is_pid_alive(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as i32, 0) };
    if ret == 0 {
        return true;
    }
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    errno == libc::EPERM
}
