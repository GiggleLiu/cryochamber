// src/process.rs
use anyhow::{Context, Result};
use std::path::Path;

// ── Cross-platform signal constants ─────────────────────────────────────────

/// SIGTERM: graceful termination request.
#[cfg(unix)]
pub const SIGTERM: i32 = libc::SIGTERM;
#[cfg(windows)]
pub const SIGTERM: i32 = 15;

/// SIGKILL: forceful termination.
#[cfg(unix)]
pub const SIGKILL: i32 = libc::SIGKILL;
#[cfg(windows)]
pub const SIGKILL: i32 = 9;

/// SIGUSR1: user-defined signal (Unix only; defined as constant for cross-platform compilation).
#[cfg(unix)]
pub const SIGUSR1: i32 = libc::SIGUSR1;
#[cfg(windows)]
pub const SIGUSR1: i32 = 10; // Not usable on Windows; defined only for compilation

// ── Process helpers ──────────────────────────────────────────────────────────

/// Check whether a process with the given PID is still alive.
pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let ret = unsafe { libc::kill(pid as i32, 0) };
        if ret == 0 {
            return true;
        }
        // EPERM means process exists but we lack permission — still alive
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        errno == libc::EPERM
    }
    #[cfg(windows)]
    {
        // tasklist /FI "PID eq N" /NH /FO CSV → outputs header only when no match
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"])
            .output();
        match output {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout);
                // A match line starts with a quoted image name, e.g. "cargo.exe","1234",...
                s.contains(&format!("\",\"{}\"", pid))
                    || s.contains(&format!("\"{pid}\""))
            }
            Err(_) => false,
        }
    }
}

/// Send a signal to a process. Returns true if delivered.
/// On Windows signals are not supported; use `terminate_pid` instead.
pub fn send_signal(pid: u32, signal: i32) -> bool {
    #[cfg(unix)]
    {
        let ret = unsafe { libc::kill(pid as i32, signal) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            eprintln!("Warning: failed to send signal {signal} to PID {pid}: {err}");
            false
        } else {
            true
        }
    }
    #[cfg(windows)]
    {
        let _ = (pid, signal); // suppress unused warnings
        // On Windows, signals aren't meaningful. Callers that need wake/kill
        // should use terminate_pid() or the file-based wake mechanism.
        false
    }
}

/// Gracefully terminate a process: SIGTERM, wait up to 5 s, escalate to SIGKILL.
pub fn terminate_pid(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        println!("Sending SIGTERM to process {pid}...");
        send_signal(pid, libc::SIGTERM);

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

        println!("Process {pid} did not exit, sending SIGKILL...");
        send_signal(pid, libc::SIGKILL);
        std::thread::sleep(std::time::Duration::from_millis(200));
        Ok(())
    }
    #[cfg(windows)]
    {
        println!("Terminating process {pid}...");
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .context("Failed to run taskkill")?;
        Ok(())
    }
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
