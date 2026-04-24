use super::*;

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
