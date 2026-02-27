// src/process.rs
use anyhow::{Context, Result};
use std::path::Path;

/// Send a signal to a process. Returns true if delivered, false on failure.
pub fn send_signal(pid: u32, signal: i32) -> bool {
    let ret = unsafe { libc::kill(pid as i32, signal) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        eprintln!("Warning: failed to send signal {signal} to PID {pid}: {err}");
        false
    } else {
        true
    }
}

/// Send SIGUSR1 to the daemon to force an immediate wake.
/// Returns true if the signal was delivered successfully.
pub fn signal_daemon_wake(dir: &Path) -> bool {
    if let Ok(Some(st)) = crate::state::load_state(&crate::state::state_path(dir)) {
        if let Some(pid) = st.pid {
            if crate::state::is_locked(&st) {
                return send_signal(pid, libc::SIGUSR1);
            }
        }
    }
    false
}

/// Send SIGTERM to a process, wait for it to exit, escalate to SIGKILL if needed.
pub fn terminate_pid(pid: u32) -> Result<()> {
    println!("Sending SIGTERM to process {pid}...");
    send_signal(pid, libc::SIGTERM);

    // Poll for up to 5 seconds
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let ret = unsafe { libc::kill(pid as i32, 0) };
        if ret != 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if errno != libc::EPERM {
                return Ok(()); // process is gone
            }
        }
    }

    // Escalate to SIGKILL
    println!("Process {pid} did not exit, sending SIGKILL...");
    send_signal(pid, libc::SIGKILL);
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
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
    std::process::Command::new(&exe)
        .arg("daemon")
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(err_file)
        .spawn()
        .context("Failed to spawn daemon process")?;
    Ok(())
}
