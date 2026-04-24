use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::channel::store::MessageStore;
use crate::config;
use crate::daemon_client;
use crate::state::{self, CryoState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonLaunchMode {
    BackgroundProcess,
    Service,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartOptions {
    pub agent_override: Option<String>,
    pub max_session_duration_override: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PreparedStart {
    pub effective_agent: String,
    pub state: CryoState,
}

pub fn require_valid_project(dir: &Path) -> Result<()> {
    if !config::config_path(dir).exists() {
        anyhow::bail!(
            "No cryochamber project in this directory: no cryo.toml found. Run `cryo init` first."
        );
    }
    Ok(())
}

pub fn require_live_daemon(dir: &Path) -> Result<CryoState> {
    require_valid_project(dir)?;
    let cryo_state = state::load_state(&state::state_path(dir))?
        .context("No daemon state found. Run `cryo start` first.")?;
    if !state::is_locked(&cryo_state) || !daemon_responding(dir) {
        anyhow::bail!(
            "No live daemon in this directory (stale state from a previous run). \
             Run `cryo start` to start a new one, or `cryo cancel` to clean up stale state."
        );
    }
    Ok(cryo_state)
}

pub fn prepare_start(dir: &Path, options: StartOptions) -> Result<PreparedStart> {
    require_valid_project(dir)?;

    if !dir.join("plan.md").exists() {
        anyhow::bail!("No plan.md found in the working directory. Create one or run `cryo init`.");
    }

    if let Some(existing) = state::load_state(&state::state_path(dir))? {
        if state::is_locked(&existing) {
            anyhow::bail!(
                "A cryochamber session is already running (PID: {:?}). Use `cryo cancel` to stop it first.",
                existing.pid
            );
        }
    }

    let cfg = config::load_config(&config::config_path(dir))?.unwrap_or_default();
    let effective_agent = options
        .agent_override
        .as_deref()
        .unwrap_or(&cfg.agent)
        .to_string();

    let state = CryoState {
        session_number: 0,
        pid: None,
        retry_count: 0,
        agent_override: options.agent_override,
        max_session_duration_override: options.max_session_duration_override,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        previous_session_crashed: false,
        session_active: false,
    };

    Ok(PreparedStart {
        effective_agent,
        state,
    })
}

pub fn validate_agent_command(agent_cmd: &str, extra_bin_dir: Option<&Path>) -> Result<()> {
    let program = crate::agent::agent_program(agent_cmd)?;
    let mut cmd = std::process::Command::new("which");
    cmd.arg(&program)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(bin_dir) = extra_bin_dir {
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{}", bin_dir.display(), path));
    }
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        _ => anyhow::bail!(
            "Agent command '{}' not found. Verify it is installed and on your PATH.",
            program
        ),
    }
}

pub fn launch_daemon(dir: &Path, exe: &Path) -> Result<DaemonLaunchMode> {
    if std::env::var("CRYO_NO_SERVICE").is_ok() {
        crate::process::spawn_daemon(dir, exe)?;
        Ok(DaemonLaunchMode::BackgroundProcess)
    } else {
        let log_path = crate::log::log_path(dir);
        crate::service::install("daemon", dir, exe, &["daemon"], &log_path, false)?;
        Ok(DaemonLaunchMode::Service)
    }
}

/// Stop the daemon for `dir`, leaving timer.json in place with runtime
/// overrides and session metadata preserved. This is intentionally different
/// from `cryo cancel`, which removes timer.json.
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

/// Restart the daemon for `dir` using the provided cryo executable. Preserves
/// session number and CLI overrides by clearing only the PID lock before
/// relaunching.
pub fn restart_chamber(dir: &Path, exe: &Path) -> Result<DaemonLaunchMode> {
    stop_chamber(dir)?;
    let launch_mode = launch_daemon(dir, exe)?;
    wait_for_live_daemon(dir)?;
    Ok(launch_mode)
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
/// archive directory. Sync configuration files such as `gh-sync.json`,
/// `zulip-sync.json`, and `.cryo/zuliprc` stay in place.
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

/// Reset the chamber: stop the daemon if needed, archive runtime state, and
/// recreate the message directories so any external sync daemon can keep
/// delivering into the live chamber directory.
pub fn reset_chamber(dir: &Path) -> Result<PathBuf> {
    stop_chamber(dir)?;
    let archive = archive_runtime(dir)?;
    MessageStore::new(dir.to_path_buf()).ensure_dirs()?;
    Ok(archive)
}

pub fn daemon_responding(dir: &Path) -> bool {
    daemon_client::daemon_responding(dir)
}

pub fn wait_for_live_daemon(dir: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    wait_for_live_daemon_until(dir, deadline)
}

pub fn wait_for_live_daemon_until(dir: &Path, deadline: std::time::Instant) -> Result<()> {
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
