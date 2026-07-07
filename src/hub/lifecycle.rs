//! Per-chamber lifecycle wrappers for the hub. Core chamber operations live in
//! `crate::lifecycle`; this module keeps hub-specific cryo binary resolution.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::channel::store::MessageStore;

/// Start a daemon for the chamber at `dir`. Mirrors `cmd_start` in the CLI.
pub fn start_chamber(dir: &Path) -> Result<()> {
    let exe = resolve_cryo_exe()?;
    let prepared = crate::lifecycle::prepare_start(dir, crate::lifecycle::StartOptions::default())?;
    crate::lifecycle::validate_agent_command(&prepared.effective_agent, exe.parent())?;

    MessageStore::new(dir.to_path_buf()).ensure_dirs()?;

    crate::state::save_state(&crate::state::state_path(dir), &prepared.state)?;

    crate::lifecycle::launch_daemon(dir, &exe)?;
    crate::lifecycle::wait_for_live_daemon(dir)?;
    Ok(())
}

/// Stop the daemon for the chamber at `dir`. Mirrors `cmd_cancel`, but leaves
/// timer.json intact (stop is not the same as cancel — restart needs overrides).
pub fn stop_chamber(dir: &Path) -> Result<()> {
    crate::lifecycle::stop_chamber(dir)
}

/// Restart = stop + start. Preserves overrides and session number.
pub fn restart_chamber(dir: &Path) -> Result<()> {
    let exe = resolve_cryo_exe()?;
    crate::lifecycle::restart_chamber(dir, &exe)?;
    Ok(())
}

/// Move `cryo.log` and `cryo-agent.log` into `history/<timestamp>/` within the
/// chamber dir, returning the archive directory. Missing files are skipped.
pub fn archive_logs(dir: &Path) -> Result<PathBuf> {
    crate::lifecycle::archive_logs(dir)
}

/// Move resettable runtime state into `history/<timestamp>/`, returning the
/// archive directory. Missing files are skipped.
pub fn archive_runtime(dir: &Path) -> Result<PathBuf> {
    crate::lifecycle::archive_runtime(dir)
}

/// Archive a chamber: fold it out of the hub's active grid. The daemon must be
/// stopped first — archived chambers cannot run until unarchived. Reversible
/// via `unarchive_chamber`; no files are moved. Only touches the registry flag.
pub fn archive_chamber(dir: &Path) -> Result<()> {
    if crate::chamber_status::overview(dir).running {
        anyhow::bail!("Stop the chamber before archiving it");
    }
    crate::registry::set_archived(dir, true)
}

/// Unarchive a chamber: return it to the active grid. Leaves it stopped — the
/// operator presses Launch to run it again.
pub fn unarchive_chamber(dir: &Path) -> Result<()> {
    crate::registry::set_archived(dir, false)
}

/// Reset the chamber: stop the daemon (if running) and archive runtime state
/// under `history/<timestamp>/`. Leaves the chamber stopped — the operator
/// presses Start when they want a new session. Re-creates `messages/` so
/// any still-running sync daemon (e.g. cryo-zulip) keeps delivering into the
/// live directory. Destructive; the UI must confirm before calling.
pub fn reset_chamber(dir: &Path) -> Result<PathBuf> {
    crate::lifecycle::reset_chamber(dir)
}

#[cfg(test)]
fn wait_for_live_daemon_until(dir: &Path, deadline: std::time::Instant) -> Result<()> {
    crate::lifecycle::wait_for_live_daemon_until(dir, deadline)
}

/// Resolve the path to the `cryo` binary. The hub is `cryohub`, so
/// `current_exe()` is the wrong answer — it would install the chamber daemon
/// service to run `cryohub daemon` (which expects `--host`/`--port`).
///
/// Strategy:
///   1. Look for a `cryo` binary alongside the running `cryohub` (works in dev:
///      `target/debug/cryo` next to `target/debug/cryohub`; works post-install:
///      `~/.cargo/bin/cryo` next to `~/.cargo/bin/cryohub`).
///   2. Fall back to `which cryo` for less standard layouts.
fn resolve_cryo_exe() -> Result<PathBuf> {
    let me = std::env::current_exe().context("Failed to resolve current executable path")?;
    resolve_cryo_exe_from(&me, which_cryo)
}

fn resolve_cryo_exe_from(me: &Path, path_lookup: impl Fn() -> Option<PathBuf>) -> Result<PathBuf> {
    if let Some(parent) = me.parent() {
        let sibling = parent.join("cryo");
        if sibling.is_file() {
            return Ok(sibling);
        }
        if parent.file_name().is_some_and(|name| name == "deps") {
            if let Some(debug_dir) = parent.parent() {
                let debug_sibling = debug_dir.join("cryo");
                if debug_sibling.is_file() {
                    return Ok(debug_sibling);
                }
            }
        }
    }
    if let Some(path_cryo) = path_lookup() {
        return Ok(path_cryo);
    }
    anyhow::bail!(
        "Could not locate the `cryo` binary. Looked next to {} and on PATH. \
         Build with `cargo build` or install with `cargo install --path .`.",
        me.display()
    )
}

fn which_cryo() -> Option<PathBuf> {
    let output = std::process::Command::new("which").arg("cryo").output();
    if let Ok(out) = output {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "../unit_tests/hub/lifecycle.rs"]
mod tests;
