use super::*;

#[test]
fn invalid_process_ids_never_reach_signal_syscalls() {
    for pid in [0, i32::MAX as u32 + 1, u32::MAX] {
        // Probe only: this regression check is safe even before the fix.
        assert!(!send_signal(pid, 0));
        assert!(!send_signal_group(pid, 0));
        assert!(!is_pid_alive(pid));
        assert!(terminate_pid(pid).is_err());
    }
    assert!(!send_signal_group(1, 0));
    assert!(is_pid_alive(std::process::id()));
}

#[test]
fn pid_probe_indicates_alive_for_live_process() {
    assert!(pid_probe_indicates_alive(0, 0));
}

#[test]
fn pid_probe_indicates_alive_when_permission_denied() {
    assert!(pid_probe_indicates_alive(-1, libc::EPERM));
}

#[test]
fn pid_probe_indicates_dead_for_missing_process() {
    assert!(!pid_probe_indicates_alive(-1, libc::ESRCH));
}

#[test]
fn send_signal_group_null_signal_probes_own_group() {
    // Signal 0 is the POSIX existence/permission probe — no signal is
    // delivered. Targeting our own process group (a live group we belong to)
    // must succeed, proving the negative-pid group addressing is well-formed.
    let pgid = unsafe { libc::getpgrp() } as u32;
    assert!(send_signal_group(pgid, 0));
}

#[test]
fn send_signal_group_kills_group_leader() {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::Command;

    // Spawn a child in its own process group (pgid == child pid), exactly how
    // `spawn_agent` launches the agent. Signaling the group must reach it.
    let mut child = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("spawn sleep");
    let pgid = child.id();

    // The live group is signalable via the negative-pid form.
    assert!(
        send_signal_group(pgid, 0),
        "live group should be signalable"
    );

    // SIGKILL the whole group and confirm the leader actually died from it.
    assert!(send_signal_group(pgid, libc::SIGKILL));
    let status = child.wait().expect("wait for killed child");
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "group SIGKILL should terminate the group leader"
    );
}
