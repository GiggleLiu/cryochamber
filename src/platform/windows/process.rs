use anyhow::Result;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};

/// Check if a process is alive using OpenProcess + GetExitCodeProcess.
pub fn is_alive(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        // STILL_ACTIVE = 259
        ok != 0 && exit_code == 259
    }
}

/// Send a signal to a process. No-op on Windows — signals are not used.
pub fn send_signal(_pid: u32, _signal: i32) -> bool {
    false
}

/// Terminate a process: wait up to 5s, then force-kill.
/// On Windows, graceful shutdown is handled via IPC by the caller.
pub fn terminate(pid: u32) -> Result<()> {
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !is_alive(pid) {
            return Ok(());
        }
    }
    force_kill(pid)
}

/// Force kill a process using TerminateProcess.
pub fn force_kill(pid: u32) -> Result<()> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            anyhow::bail!("Failed to open process {pid} for termination");
        }
        let ok = TerminateProcess(handle, 1);
        CloseHandle(handle);
        if ok == 0 {
            anyhow::bail!("TerminateProcess failed for PID {pid}");
        }
    }
    Ok(())
}

/// Terminate a child process. On Windows, use the Child handle directly.
pub fn terminate_child(child: &mut std::process::Child, _pid: u32) {
    let _ = child.kill();
    let _ = child.wait();
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
        // PID 4_000_000 is unlikely to exist
        assert!(!is_alive(4_000_000));
    }
}
