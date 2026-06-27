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

/// Send a signal to the entire process group led by `pgid` (negative-PID `kill`).
/// Agents are spawned as their own group leader (pgid == the agent's pid), so this
/// reaps the whole subtree — the wrapper script, the real CLI it launches, and any
/// grandchildren it spawned. Returns true if delivered, false on failure.
pub fn send_signal_group(pgid: u32, signal: i32) -> bool {
    let ret = unsafe { libc::kill(-(pgid as i32), signal) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        eprintln!("Warning: failed to send signal {signal} to process group {pgid}: {err}");
        false
    } else {
        true
    }
}

pub(crate) fn pid_probe_indicates_alive(ret: i32, errno: i32) -> bool {
    ret == 0 || errno == libc::EPERM
}

/// Send SIGTERM to a process, wait for it to exit, escalate to SIGKILL if needed.
pub fn terminate_pid(pid: u32) -> Result<()> {
    println!("Sending SIGTERM to process {pid}...");
    send_signal(pid, libc::SIGTERM);

    // Poll for up to 5 seconds
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let ret = unsafe { libc::kill(pid as i32, 0) };
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if !pid_probe_indicates_alive(ret, errno) {
            return Ok(()); // process is gone
        }
    }

    // Escalate to SIGKILL
    println!("Process {pid} did not exit, sending SIGKILL...");
    send_signal(pid, libc::SIGKILL);
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}

/// Spawn the `cryo daemon` subprocess in the background.
///
/// `exe` must be the path to the `cryo` binary. Callers are responsible for
/// resolving it — when the caller is itself `cryo`, `std::env::current_exe()`
/// works; from another binary (e.g. `cryohub`), use a sibling/PATH lookup.
pub fn spawn_daemon(dir: &Path, exe: &Path) -> Result<()> {
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("cryo.log"))
        .context("Failed to open cryo.log")?;
    let err_file = log_file.try_clone().context("Failed to clone log handle")?;
    std::process::Command::new(exe)
        .arg("daemon")
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(err_file)
        .spawn()
        .context("Failed to spawn daemon process")?;
    Ok(())
}

#[cfg(test)]
#[path = "unit_tests/process.rs"]
mod tests;
