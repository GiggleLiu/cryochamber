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
    /// Hub display state: archived chambers are folded away in the dashboard
    /// and cannot be started until unarchived. Purely a Cryohub concern — the
    /// CLI ignores it. Only `set_archived` mutates this; `register`,
    /// `unregister`, and `remember_chamber` preserve whatever is on disk so a
    /// daemon start/stop cycle never clears the flag.
    #[serde(default)]
    pub archived: bool,
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

/// Canonicalize a chamber directory so symlinked path forms (e.g. macOS
/// `/var` → `/private/var`) resolve to a single registry entry. Discovery keys
/// chambers by their canonical path, so registry writes must agree or the same
/// chamber ends up with two entries. Falls back to the raw path when the
/// directory does not exist yet (e.g. pruning tests).
fn canonical_dir(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
}

/// Stable filename for a given working directory. The directory is
/// canonicalized first so every path form of the same chamber hashes alike.
fn entry_filename(dir: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical_dir(dir).hash(&mut hasher);
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
            archived: read_archived(dir),
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
            archived: read_archived(dir),
        },
    )
}

/// Read the persisted `archived` flag for a chamber, defaulting to `false`
/// when there is no entry yet or it cannot be parsed. Used to carry the flag
/// across `register`/`unregister`/`remember_chamber`, which otherwise rewrite
/// the whole entry.
fn read_archived(dir: &Path) -> bool {
    read_entry(dir).map(|e| e.archived).unwrap_or(false)
}

/// Return whether a chamber is archived in the hub registry.
pub fn is_archived(dir: &Path) -> bool {
    read_archived(dir)
}

/// Read the current on-disk entry for a chamber, if any.
fn read_entry(dir: &Path) -> Option<DaemonEntry> {
    let reg = registry_dir().ok()?;
    let path = reg.join(entry_filename(dir));
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Set the hub `archived` flag for a chamber, preserving all other fields.
/// Creates a minimal entry if none exists yet.
pub fn set_archived(dir: &Path, archived: bool) -> Result<()> {
    let mut entry = read_entry(dir).unwrap_or_else(|| DaemonEntry {
        pid: None,
        dir: dir.to_string_lossy().to_string(),
        socket_path: None,
        archived: false,
    });
    entry.archived = archived;
    write_entry(dir, entry)
}

fn write_entry(dir: &Path, entry: DaemonEntry) -> Result<()> {
    let reg = registry_dir()?;
    let path = reg.join(entry_filename(dir));
    std::fs::write(&path, serde_json::to_string(&entry)?)?;
    Ok(())
}

/// Read-only content fingerprint of the whole registry: every entry file's
/// name and bytes folded into one hash. Unlike [`list`], which repairs the
/// store as it reads (rewriting entries, pruning dead ones), this changes
/// nothing on disk — so the hub's registry watch can poll it and have its own
/// refresh's repair writes compare equal instead of retriggering forever.
pub fn fingerprint() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let Ok(reg) = registry_dir() else {
        return 0;
    };
    let mut entries: Vec<(String, String)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&reg) {
        for file in rd.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let name = file.file_name().to_string_lossy().into_owned();
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            entries.push((name, content));
        }
    }
    entries.sort();
    entries.hash(&mut hasher);
    hasher.finish()
}

/// Mark this daemon as stopped while preserving the chamber entry.
pub fn unregister(dir: &Path) {
    let entry = DaemonEntry {
        pid: None,
        dir: dir.to_string_lossy().to_string(),
        socket_path: None,
        archived: read_archived(dir),
    };
    let _ = write_entry(dir, entry);
}

/// List all remembered chambers. Missing chamber directories are pruned;
/// dead PIDs are cleared so the entry remains visible as stopped.
pub fn list() -> Result<Vec<DaemonEntry>> {
    struct Candidate {
        entry: DaemonEntry,
        file_path: PathBuf,
        canonical_file: PathBuf,
        is_canonical_file: bool,
    }

    let reg = registry_dir()?;
    let mut entries = std::collections::BTreeMap::<PathBuf, Candidate>::new();
    let mut remove_paths = Vec::new();

    let dir = match std::fs::read_dir(&reg) {
        Ok(dir) => dir,
        Err(_) => return Ok(Vec::new()),
    };

    for file in dir {
        let file = file?;
        let file_path = file.path();
        if file_path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut entry: DaemonEntry = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(_) => {
                let _ = std::fs::remove_file(&file_path);
                continue;
            }
        };

        let chamber_dir = PathBuf::from(&entry.dir);
        if !crate::config::config_path(&chamber_dir).exists() {
            let _ = std::fs::remove_file(&file_path);
            continue;
        }

        if let Some(pid) = entry.pid {
            if !is_pid_alive(pid) {
                entry.pid = None;
                entry.socket_path = None;
            }
        }

        let canonical = canonical_dir(&chamber_dir);
        let canonical_file = reg.join(entry_filename(&canonical));
        let candidate = Candidate {
            entry,
            is_canonical_file: file_path == canonical_file,
            file_path,
            canonical_file,
        };

        match entries.entry(canonical) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(candidate);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                if candidate.is_canonical_file && !slot.get().is_canonical_file {
                    remove_paths.push(slot.get().file_path.clone());
                    slot.insert(candidate);
                } else {
                    remove_paths.push(candidate.file_path);
                }
            }
        }
    }

    let mut out = Vec::new();
    for candidate in entries.into_values() {
        if candidate.file_path != candidate.canonical_file {
            std::fs::write(
                &candidate.canonical_file,
                serde_json::to_string(&candidate.entry)?,
            )?;
            remove_paths.push(candidate.file_path);
        } else {
            std::fs::write(
                &candidate.file_path,
                serde_json::to_string(&candidate.entry)?,
            )?;
        }
        out.push(candidate.entry);
    }

    for path in remove_paths {
        let _ = std::fs::remove_file(path);
    }

    Ok(out)
}

fn is_pid_alive(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as i32, 0) };
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    crate::process::pid_probe_indicates_alive(ret, errno)
}

#[cfg(test)]
#[path = "unit_tests/registry.rs"]
mod tests;
