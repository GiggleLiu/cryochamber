use anyhow::{Context, Result};
use std::path::Path;

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
    pub max_retries_override: Option<u32>,
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
        max_retries_override: options.max_retries_override,
        max_session_duration_override: options.max_session_duration_override,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
        in_flight_fallback: None,
        previous_session_crashed: false,
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
