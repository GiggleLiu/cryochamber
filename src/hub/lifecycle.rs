//! Per-chamber lifecycle wrappers: start, stop, restart. These reproduce the
//! paths in `cryo start` / `cryo cancel` / `cryo restart` (see `src/bin/cryo.rs`)
//! but take an explicit `dir: &Path` and do not read the process-wide `work_dir()`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::state::{self, CryoState};

/// Start a daemon for the chamber at `dir`. Mirrors `cmd_start` in the CLI.
pub fn start_chamber(dir: &Path) -> Result<()> {
    if !crate::config::config_path(dir).exists() {
        anyhow::bail!("Not a chamber: no cryo.toml in {}", dir.display());
    }
    if !dir.join("plan.md").exists() {
        anyhow::bail!("Missing plan.md in {}", dir.display());
    }

    if let Some(existing) = state::load_state(&state::state_path(dir))? {
        if state::is_locked(&existing) {
            anyhow::bail!("A daemon is already running in {}", dir.display());
        }
    }

    let cfg = crate::config::load_config(&crate::config::config_path(dir))?.unwrap_or_default();
    validate_agent_command(&cfg.agent)?;

    crate::message::ensure_dirs(dir)?;

    let cryo_state = CryoState {
        session_number: 0,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
    };
    state::save_state(&state::state_path(dir), &cryo_state)?;

    launch_daemon(dir)?;
    wait_for_live_daemon(dir)?;
    Ok(())
}

/// Stop the daemon for the chamber at `dir`. Mirrors `cmd_cancel`, but leaves
/// timer.json intact (stop is not the same as cancel — restart needs overrides).
pub fn stop_chamber(dir: &Path) -> Result<()> {
    let _ = crate::service::uninstall("daemon", dir);
    if let Some(st) = state::load_state(&state::state_path(dir))? {
        if state::is_locked(&st) {
            if let Some(pid) = st.pid {
                crate::process::terminate_pid(pid)?;
            }
        }
        let updated = CryoState { pid: None, ..st };
        state::save_state(&state::state_path(dir), &updated)?;
    }
    Ok(())
}

/// Restart = stop + start. Preserves overrides and session number.
pub fn restart_chamber(dir: &Path) -> Result<()> {
    stop_chamber(dir)?;
    // `stop_chamber` cleared the PID lock, so launching again is safe.
    launch_daemon(dir)?;
    wait_for_live_daemon(dir)?;
    Ok(())
}

fn new_archive_dir(dir: &Path) -> Result<PathBuf> {
    let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let archive = dir.join("history").join(&ts);
    std::fs::create_dir_all(&archive)
        .with_context(|| format!("Failed to create {}", archive.display()))?;
    Ok(archive)
}

fn move_into_archive(dir: &Path, archive: &Path, name: &str) -> Result<()> {
    let src = dir.join(name);
    if !src.exists() {
        return Ok(());
    }
    let dst = archive.join(name);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    std::fs::rename(&src, &dst)
        .with_context(|| format!("Failed to move {} to {}", src.display(), dst.display()))?;
    Ok(())
}

/// Move `cryo.log` and `cryo-agent.log` into `history/<timestamp>/` within the
/// chamber dir, returning the archive directory. Missing files are skipped.
pub fn archive_logs(dir: &Path) -> Result<PathBuf> {
    let archive = new_archive_dir(dir)?;
    for name in ["cryo.log", "cryo-agent.log"] {
        move_into_archive(dir, &archive, name)?;
    }
    Ok(archive)
}

/// Move resettable runtime state into `history/<timestamp>/`, returning the
/// archive directory. Missing files are skipped.
pub fn archive_runtime(dir: &Path) -> Result<PathBuf> {
    let archive = new_archive_dir(dir)?;
    for name in [
        "cryo.log",
        "cryo-agent.log",
        "todo.json",
        "NOTES.md",
        "messages",
        "timer.json",
    ] {
        move_into_archive(dir, &archive, name)?;
    }
    Ok(archive)
}

/// Reset the chamber: stop the daemon (if running) and archive runtime state
/// under `history/<timestamp>/`. Leaves the chamber stopped — the operator
/// presses Start when they want a new session. Re-creates `messages/` so
/// any still-running sync daemon (e.g. cryo-zulip) keeps delivering into the
/// live directory. Destructive; the UI must confirm before calling.
pub fn reset_chamber(dir: &Path) -> Result<PathBuf> {
    stop_chamber(dir)?;
    let archive = archive_runtime(dir)?;
    crate::message::ensure_dirs(dir)?;
    Ok(archive)
}

fn launch_daemon(dir: &Path) -> Result<()> {
    let exe = resolve_cryo_exe()?;
    if std::env::var("CRYO_NO_SERVICE").is_ok() {
        crate::process::spawn_daemon(dir, &exe)?;
    } else {
        let log_path = crate::log::log_path(dir);
        crate::service::install("daemon", dir, &exe, &["daemon"], &log_path, false)?;
    }
    Ok(())
}

/// Block until the daemon for `dir` has locked its PID in `timer.json` and is
/// answering on its IPC socket. Mirrors the CLI `cmd_start` behaviour so the
/// `app.refresh()` that follows reads a live, not stale, state. Bails after
/// 10 seconds.
fn wait_for_live_daemon(dir: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    wait_for_live_daemon_until(dir, deadline)
}

fn wait_for_live_daemon_until(dir: &Path, deadline: std::time::Instant) -> Result<()> {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(st) = state::load_state(&state::state_path(dir))? {
            if state::is_locked(&st) && daemon_responding(dir) {
                return Ok(());
            }
        }
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Daemon did not start within 10 seconds. Check cryo.log for errors.");
        }
    }
}

fn daemon_responding(dir: &Path) -> bool {
    matches!(
        crate::socket::send_request(dir, &crate::socket::Request::Ping),
        Ok(resp) if resp.ok
    )
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

fn validate_agent_command(agent_cmd: &str) -> Result<()> {
    let program = crate::agent::agent_program(agent_cmd)?;
    let status = std::process::Command::new("which")
        .arg(&program)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => anyhow::bail!("Agent command '{}' not found on PATH", program),
    }
}

#[cfg(test)]
#[path = "../unit_tests/hub/lifecycle.rs"]
mod tests;
