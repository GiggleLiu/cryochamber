//! Unix process management primitives.
//!
//! Wraps `libc::kill`-based process queries and signal delivery so that
//! higher-level code never touches `libc` directly.

use std::time::Duration;

/// Check if a process is alive using `kill(pid, 0)`.
///
/// Returns `true` if the process exists (or exists but we lack permission),
/// `false` if it is gone.
pub fn is_alive(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as i32, 0) };
    if ret == 0 {
        return true;
    }
    // EPERM means process exists but we lack permission — still alive
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    errno == libc::EPERM
}

/// Send a signal to a process. Returns `true` if delivered successfully.
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

/// Send SIGTERM to a process, wait up to 5 seconds for it to exit,
/// then escalate to SIGKILL.
pub fn terminate(pid: u32) -> anyhow::Result<()> {
    println!("Sending SIGTERM to process {pid}...");
    send_signal(pid, libc::SIGTERM);

    // Poll for up to 5 seconds
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if !is_alive(pid) {
            return Ok(());
        }
    }

    // Escalate to SIGKILL
    println!("Process {pid} did not exit, sending SIGKILL...");
    send_signal(pid, libc::SIGKILL);
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

/// Force kill a process with SIGKILL.
pub fn force_kill(pid: u32) -> anyhow::Result<()> {
    if !send_signal(pid, libc::SIGKILL) {
        anyhow::bail!("Failed to send SIGKILL to PID {pid}");
    }
    Ok(())
}

/// Gracefully terminate a child process: SIGTERM, wait 2 seconds, SIGKILL.
///
/// Always reaps the child to prevent zombies.
pub fn terminate_child(child: &mut std::process::Child, pid: u32) {
    send_signal(pid, libc::SIGTERM);
    std::thread::sleep(Duration::from_secs(2));
    if child.try_wait().ok().flatten().is_none() {
        send_signal(pid, libc::SIGKILL);
    }
    let _ = child.wait(); // reap to prevent zombie
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_alive_current_process() {
        assert!(is_alive(std::process::id()));
    }

    #[test]
    fn test_is_alive_nonexistent() {
        assert!(!is_alive(4_000_000));
    }

    #[test]
    fn test_send_signal_to_self() {
        // Signal 0 is a no-op probe — should succeed for our own process
        assert!(send_signal(std::process::id(), 0));
    }

    #[test]
    fn test_send_signal_to_nonexistent() {
        assert!(!send_signal(4_000_000, 0));
    }
}
