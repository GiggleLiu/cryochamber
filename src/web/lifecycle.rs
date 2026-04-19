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
    launch_daemon(dir)
}

/// Move `cryo.log` and `cryo-agent.log` into `history/<timestamp>/` within the
/// chamber dir, returning the archive directory. Missing files are skipped.
pub fn archive_logs(dir: &Path) -> Result<PathBuf> {
    let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let archive = dir.join("history").join(&ts);
    std::fs::create_dir_all(&archive)
        .with_context(|| format!("Failed to create {}", archive.display()))?;
    for name in ["cryo.log", "cryo-agent.log"] {
        let src = dir.join(name);
        if !src.exists() {
            continue;
        }
        let dst = archive.join(name);
        std::fs::rename(&src, &dst)
            .with_context(|| format!("Failed to move {} to {}", src.display(), dst.display()))?;
    }
    Ok(archive)
}

/// Reset the chamber: stop the daemon (if running), archive logs under
/// `history/<timestamp>/`, then start a fresh session. Destructive — the UI
/// must confirm before calling. Returns the archive directory path.
pub fn reset_chamber(dir: &Path) -> Result<PathBuf> {
    stop_chamber(dir)?;
    let archive = archive_logs(dir)?;
    start_chamber(dir)?;
    Ok(archive)
}

fn launch_daemon(dir: &Path) -> Result<()> {
    if std::env::var("CRYO_NO_SERVICE").is_ok() {
        crate::process::spawn_daemon(dir)?;
    } else {
        let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
        let log_path = crate::log::log_path(dir);
        crate::service::install("daemon", dir, &exe, &["daemon"], &log_path, false)?;
    }
    Ok(())
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
mod tests {
    use super::*;

    #[test]
    fn start_chamber_rejects_missing_cryo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let err = start_chamber(dir.path()).unwrap_err();
        assert!(err.to_string().contains("no cryo.toml"));
    }

    #[test]
    fn start_chamber_rejects_missing_plan_md() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&crate::config::config_path(dir.path()), &cfg).unwrap();
        let err = start_chamber(dir.path()).unwrap_err();
        assert!(err.to_string().contains("plan.md"));
    }

    #[test]
    fn stop_chamber_is_idempotent_on_nothing_running() {
        let dir = tempfile::tempdir().unwrap();
        stop_chamber(dir.path()).unwrap();
    }

    #[test]
    fn archive_logs_moves_existing_logs_and_skips_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cryo_log = dir.path().join("cryo.log");
        std::fs::write(&cryo_log, b"old session data").unwrap();
        // cryo-agent.log intentionally absent

        let archive = archive_logs(dir.path()).unwrap();
        assert!(archive.starts_with(dir.path().join("history")));
        assert!(archive.join("cryo.log").exists());
        assert!(!archive.join("cryo-agent.log").exists());
        assert!(!cryo_log.exists(), "original cryo.log should be moved");
        assert_eq!(
            std::fs::read_to_string(archive.join("cryo.log")).unwrap(),
            "old session data"
        );
    }

    #[test]
    fn archive_logs_creates_history_dir_when_no_logs_present() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_logs(dir.path()).unwrap();
        assert!(archive.is_dir());
        assert!(dir.path().join("history").is_dir());
    }
}
