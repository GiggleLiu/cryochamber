// tests/launchd_tests.rs
#[cfg(target_os = "macos")]
mod tests {
    use chrono::NaiveDateTime;
    use cryochamber::timer::launchd::LaunchdTimer;

    #[test]
    fn test_plist_generation() {
        let timer = LaunchdTimer::new();
        let wake_time =
            NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        let plist_content = timer
            .generate_plist(
                "com.cryochamber.test",
                &wake_time,
                "/usr/local/bin/cryochamber wake",
                "/tmp/test",
            )
            .unwrap();
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
        assert!(path
            .to_string_lossy()
            .contains("com.cryochamber.test.plist"));
    }

    #[test]
    fn test_plist_quoted_args() {
        let timer = LaunchdTimer::new();
        let wake_time =
            NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        let plist_content = timer
            .generate_plist(
                "com.cryochamber.test",
                &wake_time,
                r#"cryochamber fallback-exec email user@ex.com "task failed""#,
                "/tmp/test",
            )
            .unwrap();
        assert!(plist_content.contains("<string>task failed</string>"));
        assert!(plist_content.contains("<string>user@ex.com</string>"));
    }

    #[test]
    fn test_label_generation() {
        let label = LaunchdTimer::make_label("/Users/me/plans/myproject");
        assert!(label.starts_with("com.cryochamber."));
        // Same path should produce same label
        let label2 = LaunchdTimer::make_label("/Users/me/plans/myproject");
        assert_eq!(label, label2);
    }

    #[test]
    fn test_plist_midnight_time() {
        let timer = LaunchdTimer::new();
        let wake_time =
            NaiveDateTime::parse_from_str("2025-01-15T00:00", "%Y-%m-%dT%H:%M").unwrap();
        let plist_content = timer
            .generate_plist(
                "com.cryochamber.midnight",
                &wake_time,
                "cryochamber wake",
                "/tmp/test",
            )
            .unwrap();
        // Hour=0, Minute=0 should produce integer 0
        assert!(plist_content.contains("<key>Hour</key>\n        <integer>0</integer>"));
        assert!(plist_content.contains("<key>Minute</key>\n        <integer>0</integer>"));
    }

    #[test]
    fn test_plist_end_of_year() {
        let timer = LaunchdTimer::new();
        let wake_time =
            NaiveDateTime::parse_from_str("2025-12-31T23:59", "%Y-%m-%dT%H:%M").unwrap();
        let plist_content = timer
            .generate_plist(
                "com.cryochamber.eoy",
                &wake_time,
                "cryochamber wake",
                "/tmp/test",
            )
            .unwrap();
        assert!(plist_content.contains("<integer>12</integer>")); // Month
        assert!(plist_content.contains("<integer>31</integer>")); // Day
        assert!(plist_content.contains("<integer>23</integer>")); // Hour
        assert!(plist_content.contains("<integer>59</integer>")); // Minute
    }

    #[test]
    fn test_generate_plist_unmatched_quote() {
        let timer = LaunchdTimer::new();
        let wake_time =
            NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        let result = timer.generate_plist(
            "com.cryochamber.test",
            &wake_time,
            r#"cryochamber "unclosed"#,
            "/tmp/test",
        );
        assert!(result.is_err());
    }
}
