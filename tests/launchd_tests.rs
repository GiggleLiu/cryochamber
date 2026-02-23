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
                "/usr/local/bin/cryo wake",
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
                r#"cryo fallback-exec email user@ex.com "task failed""#,
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
                "cryo wake",
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
            .generate_plist("com.cryochamber.eoy", &wake_time, "cryo wake", "/tmp/test")
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
            r#"cryo "unclosed"#,
            "/tmp/test",
        );
        assert!(result.is_err());
    }

    // --- Real timer lifecycle tests ---
    // These interact with the real launchd daemon: schedule → verify → cancel.
    // The timer is set far in the future so it never fires during the test.

    use cryochamber::timer::{CryoTimer, TimerId, TimerStatus};

    /// Helper that guarantees cleanup even if a test panics.
    struct TimerGuard<'a> {
        timer: &'a LaunchdTimer,
        id: Option<TimerId>,
    }

    impl<'a> TimerGuard<'a> {
        fn new(timer: &'a LaunchdTimer) -> Self {
            Self { timer, id: None }
        }
        fn set(&mut self, id: TimerId) {
            self.id = Some(id);
        }
    }

    impl Drop for TimerGuard<'_> {
        fn drop(&mut self) {
            if let Some(id) = &self.id {
                let _ = self.timer.cancel(id);
            }
        }
    }

    #[test]
    #[ignore] // requires real launchd GUI session — run with: cargo test -- --ignored
    fn test_launchd_schedule_verify_cancel() {
        let timer = LaunchdTimer::new();
        let mut guard = TimerGuard::new(&timer);

        // Schedule 1 year in the future — will never fire
        let wake_time =
            NaiveDateTime::parse_from_str("2099-06-15T12:00", "%Y-%m-%dT%H:%M").unwrap();
        let work_dir = "/tmp/cryochamber-test-lifecycle";

        let id = timer
            .schedule_wake(wake_time, "echo wake", work_dir)
            .expect("schedule_wake should succeed");
        guard.set(id.clone());

        // Verify it's scheduled
        let status = timer.verify(&id).expect("verify should succeed");
        assert!(
            matches!(status, TimerStatus::Scheduled { .. }),
            "Expected Scheduled, got NotFound"
        );

        // Cancel it
        timer.cancel(&id).expect("cancel should succeed");
        guard.id = None; // already cancelled

        // Verify it's gone
        let status = timer
            .verify(&id)
            .expect("verify after cancel should succeed");
        assert!(
            matches!(status, TimerStatus::NotFound),
            "Expected NotFound after cancel"
        );
    }

    #[test]
    #[ignore] // requires real launchd GUI session — run with: cargo test -- --ignored
    fn test_launchd_schedule_fallback_and_cancel() {
        let timer = LaunchdTimer::new();
        let mut guard = TimerGuard::new(&timer);

        let wake_time =
            NaiveDateTime::parse_from_str("2099-06-15T13:00", "%Y-%m-%dT%H:%M").unwrap();
        let action = cryochamber::fallback::FallbackAction {
            action: "email".to_string(),
            target: "test@example.com".to_string(),
            message: "test fallback".to_string(),
        };
        let work_dir = "/tmp/cryochamber-test-fallback";

        let id = timer
            .schedule_fallback(wake_time, &action, work_dir)
            .expect("schedule_fallback should succeed");
        guard.set(id.clone());

        let status = timer.verify(&id).expect("verify should succeed");
        assert!(matches!(status, TimerStatus::Scheduled { .. }));

        timer.cancel(&id).expect("cancel should succeed");
        guard.id = None;
    }

    #[test]
    fn test_launchd_cancel_nonexistent() {
        let timer = LaunchdTimer::new();
        // Cancelling a timer that was never registered should not error
        let fake_id = TimerId("com.cryochamber.nonexistent.test.xyz".to_string());
        let result = timer.cancel(&fake_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_launchd_verify_nonexistent() {
        let timer = LaunchdTimer::new();
        let fake_id = TimerId("com.cryochamber.nonexistent.test.xyz".to_string());
        let status = timer.verify(&fake_id).expect("verify should succeed");
        assert!(matches!(status, TimerStatus::NotFound));
    }

    #[test]
    fn test_create_timer_returns_impl() {
        let timer = cryochamber::timer::create_timer();
        assert!(timer.is_ok(), "create_timer should succeed on macOS");
    }
}
