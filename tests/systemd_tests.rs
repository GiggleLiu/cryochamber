// tests/systemd_tests.rs
// Systemd unit generation tests — these test pure string generation,
// not actual systemctl calls, so they can run on any platform.

use chrono::NaiveDateTime;
use cryochamber::timer::systemd::SystemdTimer;

#[test]
fn test_timer_unit_format() {
    let timer = SystemdTimer::new();
    let wake_time = NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
    let content = timer.generate_timer_unit("cryochamber-test", &wake_time);
    assert!(content.contains("OnCalendar=2025-03-08 09:00:00"));
    assert!(content.contains("Persistent=true"));
    assert!(content.contains("RemainAfterElapse=false"));
    assert!(content.contains("Description=Cryochamber wake timer: cryochamber-test"));
    assert!(content.contains("WantedBy=timers.target"));
}

#[test]
fn test_service_unit_format() {
    let timer = SystemdTimer::new();
    let content = timer.generate_service_unit(
        "cryochamber-test",
        "cryochamber wake",
        "/home/user/plans/myproject",
    );
    assert!(content.contains("Type=oneshot"));
    assert!(content.contains("ExecStart=cryochamber wake"));
    assert!(content.contains("WorkingDirectory=/home/user/plans/myproject"));
    assert!(content.contains("Description=Cryochamber task: cryochamber-test"));
}

#[test]
fn test_unit_name_deterministic() {
    let name = SystemdTimer::make_unit_name("/home/user/plans/myproject");
    assert!(name.starts_with("cryochamber-"));
    // Same input produces same name
    let name2 = SystemdTimer::make_unit_name("/home/user/plans/myproject");
    assert_eq!(name, name2);
    // Different input produces different name
    let name3 = SystemdTimer::make_unit_name("/home/user/plans/other");
    assert_ne!(name, name3);
}
