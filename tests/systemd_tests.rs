// tests/systemd_tests.rs
#[cfg(target_os = "linux")]
mod tests {
    use cryochamber::timer::systemd::SystemdTimer;
    use chrono::NaiveDateTime;

    #[test]
    fn test_timer_unit_generation() {
        let timer = SystemdTimer::new();
        let wake_time = NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        let content = timer.generate_timer_unit("cryochamber-test", &wake_time);
        assert!(content.contains("OnCalendar=2025-03-08 09:00:00"));
        assert!(content.contains("Persistent=true"));
        assert!(content.contains("RemainAfterElapse=false"));
    }

    #[test]
    fn test_service_unit_generation() {
        let timer = SystemdTimer::new();
        let content = timer.generate_service_unit(
            "cryochamber-test",
            "cryochamber wake",
            "/home/user/plans/myproject",
        );
        assert!(content.contains("Type=oneshot"));
        assert!(content.contains("ExecStart=cryochamber wake"));
        assert!(content.contains("WorkingDirectory=/home/user/plans/myproject"));
    }

    #[test]
    fn test_unit_name_generation() {
        let name = SystemdTimer::make_unit_name("/home/user/plans/myproject");
        assert!(name.starts_with("cryochamber-"));
        let name2 = SystemdTimer::make_unit_name("/home/user/plans/myproject");
        assert_eq!(name, name2);
    }
}
