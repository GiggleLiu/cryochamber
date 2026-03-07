// src/process.rs
use anyhow::{Context, Result};
use std::path::Path;

/// Send a signal to a process. Returns true if delivered, false on failure.
pub fn send_signal(pid: u32, signal: i32) -> bool {
    crate::platform::process::send_signal(pid, signal)
}

/// Send a wake signal to the daemon to force an immediate wake.
/// Returns true if the signal was delivered successfully.
pub fn signal_daemon_wake(dir: &Path) -> bool {
    crate::platform::signal::signal_wake(dir)
}

/// Send SIGTERM to a process, wait for it to exit, escalate to SIGKILL if needed.
pub fn terminate_pid(pid: u32) -> Result<()> {
    crate::platform::process::terminate(pid)
}

/// Spawn the daemon subprocess in the background.
pub fn spawn_daemon(dir: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("cryo.log"))
        .context("Failed to open cryo.log")?;
    let err_file = log_file.try_clone().context("Failed to clone log handle")?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("daemon")
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(err_file);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    cmd.spawn().context("Failed to spawn daemon process")?;
    Ok(())
}
