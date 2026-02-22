// tests/launchd_tests.rs
#[cfg(target_os = "macos")]
mod tests {
    use cryochamber::timer::launchd::LaunchdTimer;
    use cryochamber::timer::CryoTimer;
    use chrono::NaiveDateTime;

    #[test]
    fn test_plist_generation() {
        let timer = LaunchdTimer::new();
        let wake_time = NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        let plist_content = timer.generate_plist(
            "com.cryochamber.test",
            &wake_time,
            "/usr/local/bin/cryochamber wake",
            "/tmp/test",
        );
        assert!(plist_content.contains("com.cryochamber.test"));
        assert!(plist_content.contains("<key>Month</key>"));
        assert!(plist_content.contains("<integer>3</integer>"));
        assert!(plist_content.contains("<key>Day</key>"));
        assert!(plist_content.contains("<integer>8</integer>"));
        assert!(plist_content.contains("<key>Hour</key>"));
        assert!(plist_content.contains("<integer>9</integer>"));
        assert!(plist_content.contains("<key>Minute</key>"));
        assert!(plist_content.contains("<integer>0</integer>"));
    }

    #[test]
    fn test_plist_path() {
        let timer = LaunchdTimer::new();
        let path = timer.plist_path("com.cryochamber.test");
        assert!(path.to_string_lossy().contains("LaunchAgents"));
        assert!(path.to_string_lossy().contains("com.cryochamber.test.plist"));
    }

    #[test]
    fn test_label_generation() {
        let label = LaunchdTimer::make_label("/Users/me/plans/myproject");
        assert!(label.starts_with("com.cryochamber."));
        // Same path should produce same label
        let label2 = LaunchdTimer::make_label("/Users/me/plans/myproject");
        assert_eq!(label, label2);
    }
}
