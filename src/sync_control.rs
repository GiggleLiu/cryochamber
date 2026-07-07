//! Sync backend orchestration and concrete backend dispatch.

use crate::sync_common::{SyncBackend, SyncSummary};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn summarize(backend: SyncBackend, dir: &Path) -> Option<SyncSummary> {
    match backend {
        SyncBackend::Zulip => crate::zulip_sync::summarize(dir),
    }
}

pub fn summarize_all(dir: &Path) -> Vec<SyncSummary> {
    [SyncBackend::Zulip]
        .into_iter()
        .filter_map(|b| summarize(b, dir))
        .collect()
}

fn resolve_cli(backend: SyncBackend) -> Result<PathBuf> {
    let (env_var, bin_name) = match backend {
        SyncBackend::Zulip => ("CRYO_ZULIP_CLI", "cryo-zulip"),
    };
    if let Ok(p) = std::env::var(env_var) {
        return Ok(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sibling = parent.join(bin_name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin_name}"))
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }
    anyhow::bail!("{bin_name} binary not found (tried ${env_var}, sibling of current exe, $PATH)");
}

fn run_subcommand(backend: SyncBackend, dir: &Path, sub: &str) -> Result<()> {
    let cli = resolve_cli(backend)?;
    let output = std::process::Command::new(&cli)
        .current_dir(dir)
        .arg(sub)
        .output()
        .with_context(|| format!("Failed to spawn {}", cli.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let truncated: String = stderr.chars().take(500).collect();
        anyhow::bail!(
            "{} {sub} exited with {}: {}",
            cli.display(),
            output.status,
            truncated.trim()
        );
    }
    Ok(())
}

pub fn start(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "sync")
}

pub fn stop(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "unsync")
}

pub fn pull(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "pull")
}

pub fn push(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "push")
}

/// Ground-truth running check -- reads the per-backend pid file and verifies
/// the process is alive. The hub uses this to decide what to show in the
/// sync toggle.
pub fn is_running(backend: SyncBackend, dir: &Path) -> bool {
    match backend {
        SyncBackend::Zulip => crate::zulip_sync::is_sync_running(dir),
    }
}

/// Poll `is_running` until it matches `expected` or the deadline elapses.
/// Used after start/stop so the HTTP response and following SSE status event
/// reflect the settled pid-file state.
///
/// Returns `true` if `expected` was observed before the deadline.
pub fn wait_for_state(backend: SyncBackend, dir: &Path, expected: bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if is_running(backend, dir) == expected {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
#[path = "unit_tests/sync_control.rs"]
mod tests;
