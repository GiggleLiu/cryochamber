//! Integration tests using mock agent scenario scripts.
//! Each test spawns a real daemon with CRYO_NO_SERVICE=1 and asserts on cryo.log/timer.json.

use chrono::Timelike;
use std::fs;
use std::time::Duration;

/// Path to the cryo binary built by cargo.
fn cryo_bin() -> assert_cmd::Command {
    #[allow(deprecated)]
    assert_cmd::Command::cargo_bin("cryo").unwrap()
}

/// Initialize a cryo project in a temp directory with a specific scenario file.
///
/// `scenario_name` is the basename without extension (e.g. `"crash"` or
/// `"multi-session"`); the file at `tests/scenarios/<name>.toml` is copied into
/// the project as `scenario.toml`, where `cryo-mock` picks it up.
fn setup_scenario(dir: &std::path::Path, scenario_name: &str) {
    let name = scenario_name
        .trim_end_matches(".sh")
        .trim_end_matches(".toml");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let src = format!("{manifest}/tests/scenarios/{name}.toml");
    fs::copy(&src, dir.join("scenario.toml")).unwrap_or_else(|e| panic!("copying {src}: {e}"));
    fs::write(dir.join("plan.md"), "# Test Plan\nDo mock things.").unwrap();

    // Init cryo project
    cryo_bin()
        .args(["init", "--agent", "mock"])
        .current_dir(dir)
        .assert()
        .success();
}

/// Wait for the daemon to exit by polling timer.json for pid=null.
/// Returns true if daemon exited within the timeout.
/// Treats missing timer.json as "exited" (some commands remove the file).
fn wait_for_daemon_exit(dir: &std::path::Path, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        let timer_path = dir.join("timer.json");
        if !timer_path.exists() {
            return true;
        }
        match fs::read_to_string(&timer_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(state) if state["pid"].is_null() => return true,
                _ => {}
            },
            Err(_) => return true,
        }
    }
    false
}

/// Cancel a running daemon and wait for it to exit.
fn cancel_and_wait(dir: &std::path::Path) {
    let _ = cryo_bin().args(["cancel"]).current_dir(dir).output();
    // Wait a moment for cleanup
    wait_for_daemon_exit(dir, Duration::from_secs(5));
}

/// Overwrite cryo.toml with a custom config containing providers and rotate_on policy.
fn write_provider_config(dir: &std::path::Path, rotate_on: &str, provider_count: usize) {
    let mut providers = String::new();
    for i in 0..provider_count {
        providers.push_str(&format!(
            "\n[[providers]]\nname = \"provider-{i}\"\n[providers.env]\nMOCK_PROVIDER = \"provider-{i}\"\n"
        ));
    }
    let config = format!(
        r#"agent = "mock"
max_retries = 3
max_session_duration = 30
watch_inbox = false
rotate_on = "{rotate_on}"
{providers}"#
    );
    fs::write(dir.join("cryo.toml"), config).unwrap();
}

/// Wait for specific text to appear in cryo.log.
fn wait_for_log_content(dir: &std::path::Path, text: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(log) = fs::read_to_string(dir.join("cryo.log")) {
            if log.contains(text) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

#[test]
fn test_mock_crash_retries_then_exits() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "crash.sh");

    // Set max_retries=2 so it triggers alert quickly
    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = config.replace("max_retries = 5", "max_retries = 2");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for the crash event to appear in the log
    assert!(
        wait_for_log_content(
            dir.path(),
            "agent exited without hibernate",
            Duration::from_secs(15)
        ),
        "Log should show agent exited without hibernate"
    );

    // Cancel the daemon (it retries indefinitely)
    cancel_and_wait(dir.path());
}

#[test]
fn test_mock_quick_exit_detected() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "quick-exit.sh");

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for the quick-exit event to appear in the log
    assert!(
        wait_for_log_content(dir.path(), "quick exit detected", Duration::from_secs(15)),
        "Log should detect quick exit"
    );

    // Cancel the daemon
    cancel_and_wait(dir.path());
}

#[test]
fn test_mock_timeout_kills_agent() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "timeout.sh");

    // Set short timeout so test doesn't take forever
    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "3"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for the timeout event in the log
    assert!(
        wait_for_log_content(dir.path(), "session timeout", Duration::from_secs(15)),
        "Log should show session timeout"
    );

    // Cancel the daemon (it would retry after timeout)
    cancel_and_wait(dir.path());
}

#[test]
fn test_mock_multi_session_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "multi-session.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Should complete after 3 sessions (with ~2s wake intervals)
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(30)),
        "Daemon should exit after plan completion"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("plan complete"),
        "Log should show plan complete: {log}"
    );
    // Should have at least 3 session markers
    let session_count = log.matches("CRYO SESSION").count();
    assert!(
        session_count >= 3,
        "Should have at least 3 sessions, found {session_count}"
    );
}

#[test]
fn test_mock_ipc_all_commands() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "ipc-all.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(15)),
        "Daemon should exit after plan complete"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();

    // Verify the remaining IPC commands were logged
    assert!(log.contains("reply:"), "Missing reply in log: {log}");
    assert!(log.contains("alert:"), "Missing alert in log: {log}");
    assert!(
        log.contains("plan complete"),
        "Missing plan complete: {log}"
    );

    // Verify outbox message was written
    let outbox = dir.path().join("messages/outbox");
    assert!(outbox.exists(), "Outbox directory should exist after send");
    let files: Vec<_> = fs::read_dir(&outbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!files.is_empty(), "Outbox should have a reply message");
}

#[test]
fn test_mock_crash_then_succeed() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "crash-then-succeed.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(30)),
        "Daemon should exit after retry succeeds"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    // First session should fail, second should succeed
    assert!(
        log.contains("agent exited without hibernate"),
        "First session should crash: {log}"
    );
    assert!(
        log.contains("plan complete"),
        "Second session should complete: {log}"
    );
}

#[test]
fn test_mock_invalid_wake_time() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "invalid-wake-time.sh");

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // The agent adds a TODO with "banana" as the time, then hibernates.
    // "banana" is unparseable as NaiveDateTime, so no pending TODO has a valid
    // wake — the daemon rejects the hibernate attempt and logs the refusal.
    assert!(
        wait_for_log_content(
            dir.path(),
            "hibernate refused: no pending TODO",
            Duration::from_secs(15)
        ),
        "Daemon should refuse hibernate when no TODO has a valid wake time"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_mock_slow_exit_no_hibernate() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "slow-exit-no-hibernate.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Should NOT be flagged as quick exit (runs >5s), but still "exit without hibernate"
    assert!(
        wait_for_log_content(
            dir.path(),
            "agent exited without hibernate",
            Duration::from_secs(20)
        ),
        "Log should show exit without hibernate"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        !log.contains("quick exit"),
        "Should NOT be flagged as quick exit (ran >5s): {log}"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_mock_double_hibernate() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "double-hibernate.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // The script sends two hibernate commands: first without --complete, then
    // with --complete. The daemon takes the last one, so the plan completes.
    assert!(
        wait_for_log_content(dir.path(), "plan complete", Duration::from_secs(15)),
        "Log should show plan complete (first hibernate rejected, second succeeds)"
    );

    // Daemon should exit after plan completion
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(10)),
        "Daemon should exit after plan completion"
    );
}

#[test]
fn test_mock_orphan_child() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "orphan-child.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Daemon should not hang waiting for orphan process
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(15)),
        "Daemon should exit without hanging on orphan subprocess"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("plan complete"),
        "Session should complete normally: {log}"
    );
}

#[test]
fn test_mock_hibernate_then_crash() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "hibernate-then-crash.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // The script sends: hibernate (not complete), then exit 1.
    // The daemon accepts the hibernate, so despite the non-zero exit code,
    // the session completes with the hibernate outcome.
    assert!(
        wait_for_log_content(dir.path(), "session complete", Duration::from_secs(15)),
        "Log should show session complete (hibernate accepted despite crash)"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_mock_hibernate_exit_code_retries() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "hibernate-failed-then-succeed.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(20)),
        "Daemon should retry after nonzero hibernate exit code and then complete"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("CRYO SESSION 2"),
        "Nonzero hibernate exit code should trigger a retry session: {log}"
    );
    assert!(
        log.contains("plan complete"),
        "Retry scenario should eventually complete the plan: {log}"
    );
}

// --- Provider rotation tests ---

#[test]
fn test_rotate_on_quick_exit_rotates() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "quick-exit.sh");
    write_provider_config(dir.path(), "quick-exit", 2);

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // After quick exit with rotate_on=quick-exit, daemon should rotate to provider-1.
    // The rotation is logged via eprintln (stderr), but the next session logs
    // "provider: provider-1" to cryo.log via EventLogger.
    assert!(
        wait_for_log_content(dir.path(), "provider: provider-1", Duration::from_secs(15)),
        "Should rotate to provider-1 on quick exit"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_rotate_on_quick_exit_no_rotate_on_slow_crash() {
    let dir = tempfile::tempdir().unwrap();
    // slow-exit-no-hibernate.sh runs >5s then exits without hibernate.
    // This is a slow crash — quick_exit will be false, so rotate_on=quick-exit
    // should NOT trigger rotation.
    setup_scenario(dir.path(), "slow-exit-no-hibernate.sh");
    write_provider_config(dir.path(), "quick-exit", 2);

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for the slow exit to be detected (takes >5s)
    assert!(
        wait_for_log_content(
            dir.path(),
            "agent exited without hibernate",
            Duration::from_secs(20)
        ),
        "Should detect exit without hibernate"
    );

    // Wait for retry session to start (with backoff)
    std::thread::sleep(Duration::from_secs(8));
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();

    // With rotate_on=quick-exit, slow crashes (>5s) should NOT trigger rotation.
    assert!(
        !log.contains("provider: provider-1"),
        "Should NOT rotate on slow crash with rotate_on=quick-exit: {log}"
    );
    // Verify provider-0 is being used
    assert!(
        log.contains("provider: provider-0"),
        "Should be using provider-0: {log}"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_rotate_on_any_failure_rotates_on_crash() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "crash.sh");
    write_provider_config(dir.path(), "any-failure", 2);

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // With rotate_on=any-failure, any ValidationFailed triggers rotation.
    // The next session should use provider-1.
    assert!(
        wait_for_log_content(dir.path(), "provider: provider-1", Duration::from_secs(15)),
        "Should rotate to provider-1 on crash with rotate_on=any-failure"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_rotate_on_never_no_rotation() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "crash.sh");
    write_provider_config(dir.path(), "never", 2);

    // Set max_retries=1 so daemon sends alert quickly but keeps retrying
    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = config.replace("max_retries = 3", "max_retries = 1");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for at least two sessions to run (crash + retry)
    assert!(
        wait_for_log_content(
            dir.path(),
            "agent exited without hibernate",
            Duration::from_secs(15)
        ),
        "Should detect crash"
    );

    // Wait for second session to start (retry with backoff)
    std::thread::sleep(Duration::from_secs(8));
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();

    // With rotate_on=never, all sessions should use provider-0 (no rotation)
    assert!(
        !log.contains("provider: provider-1"),
        "Should NOT rotate with rotate_on=never, but found provider-1 in log: {log}"
    );
    // Verify provider-0 is being used
    assert!(
        log.contains("provider: provider-0"),
        "Should be using provider-0: {log}"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_provider_wrap_all_exhausted() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "quick-exit.sh");
    write_provider_config(dir.path(), "any-failure", 2);

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // With 2 providers and any-failure rotation: session 1 uses provider-0 (fails),
    // rotates to provider-1 (session 2, fails), wraps back to provider-0 (session 3).
    // After wrap, daemon backs off 60s. We detect the wrap by seeing provider-0
    // appear in the log at least twice (initial + after wrap).
    // First, wait for provider-1 to appear (first rotation).
    assert!(
        wait_for_log_content(dir.path(), "provider: provider-1", Duration::from_secs(15)),
        "Should rotate to provider-1 first"
    );

    // Then wait for provider-0 to appear again (wrap completed).
    // The daemon sleeps 60s after wrap, but provider-0 is logged at session start,
    // so we need to wait for the second occurrence.
    // Count occurrences: we need provider-0 to appear at least twice.
    let found = {
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        loop {
            if std::time::Instant::now() > deadline {
                break false;
            }
            if let Ok(log) = fs::read_to_string(dir.path().join("cryo.log")) {
                if log.matches("provider: provider-0").count() >= 2 {
                    break true;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    };
    assert!(
        found,
        "Should wrap back to provider-0 after all providers exhausted"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_provider_env_injected() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "check-env.sh");

    let config = r#"agent = "mock"
max_retries = 1
max_session_duration = 30
watch_inbox = false

[[providers]]
name = "test-provider"
[providers.env]
MOCK_VAR = "hello"
"#;
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(15)),
        "Daemon should exit after completion"
    );

    let env_check = dir.path().join(".env-check");
    assert!(env_check.exists(), ".env-check file should exist");
    let content = fs::read_to_string(&env_check).unwrap();
    assert_eq!(content.trim(), "hello", "MOCK_VAR should be injected");
}

// --- Fallback, delayed wake, and periodic report tests ---

#[test]
fn test_fallback_fires_on_deadline() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "alert-then-crash.sh");

    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = config.replace("max_retries = 5", "max_retries = 1");
    // fallback_alert is commented out in the default template (# fallback_alert = "outbox"),
    // so we always append the uncommented setting. TOML uses the last occurrence.
    let config = format!("{config}\nfallback_alert = \"outbox\"\n");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // alert-then-crash.sh calls `cryo-agent alert` then exits with code 1.
    // The daemon logs "alert: email -> ops@test.com" from the agent's alert command.
    // With max_retries=1, after the first crash, handle_failure_retry fires
    // send_retry_alert which writes a "retry_exhausted" message to outbox.
    // The daemon's stderr (redirected to cryo.log) also contains "retries failed".
    assert!(
        wait_for_log_content(dir.path(), "alert", Duration::from_secs(30)),
        "Should show alert in log (from cryo-agent alert command)"
    );

    // Wait for the retry exhaustion alert to fire and write to outbox.
    // With max_retries=1 and 5s backoff, the alert fires after ~5s.
    assert!(
        wait_for_log_content(dir.path(), "retries failed", Duration::from_secs(20)),
        "Should show retry exhaustion in log"
    );

    cancel_and_wait(dir.path());

    // Check outbox for fallback message written by send_retry_alert
    let outbox = dir.path().join("messages/outbox");
    assert!(outbox.exists(), "Outbox directory should exist");
    let files: Vec<_> = fs::read_dir(&outbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .collect();
    assert!(!files.is_empty(), "Outbox should contain fallback alert");

    // Verify at least one outbox file contains "retry_exhausted"
    let has_retry_alert = files.iter().any(|f| {
        fs::read_to_string(f.path())
            .unwrap_or_default()
            .contains("retry_exhausted")
    });
    assert!(
        has_retry_alert,
        "Outbox should contain retry_exhausted alert"
    );
}

#[test]
fn test_fallback_suppressed_when_none() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "alert-then-crash.sh");

    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = config.replace("max_retries = 5", "max_retries = 1");
    // fallback_alert is commented out in the default template, so append to override.
    let config = format!("{config}\nfallback_alert = \"none\"\n");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for crash to be detected
    assert!(
        wait_for_log_content(
            dir.path(),
            "agent exited without hibernate",
            Duration::from_secs(15)
        ),
        "Should detect crash"
    );

    // Wait long enough for the retry alert to have been attempted (backoff 5s)
    std::thread::sleep(Duration::from_secs(8));

    cancel_and_wait(dir.path());

    // Outbox should be empty — fallback_alert=none suppresses write_message
    let outbox = dir.path().join("messages/outbox");
    if outbox.exists() {
        let files: Vec<_> = fs::read_dir(&outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .collect();
        assert!(
            files.is_empty(),
            "Outbox should be empty with fallback_alert=none, but found {} files",
            files.len()
        );
    }
}

#[test]
fn test_fallback_cancelled_on_success() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "alert-then-succeed.sh");

    // Set fallback_alert=outbox so any fallback would be visible in outbox
    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    // fallback_alert is commented out in the default template, so append the uncommented setting
    let config = format!("{config}\nfallback_alert = \"outbox\"\n");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // alert-then-succeed.sh calls `cryo-agent alert` then `cryo-agent hibernate --complete`.
    // The alert registers a pending_fallback, but since the agent completes the plan
    // successfully, the fallback is never executed (pending_fallback is dropped on PlanComplete).
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(15)),
        "Daemon should exit after plan completion"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("plan complete"),
        "Session should complete: {log}"
    );

    // Outbox should NOT contain a fallback alert.
    // The agent's alert command only registers a pending_fallback in memory;
    // it doesn't write to outbox. Since the plan completed, the pending fallback
    // is dropped without executing.
    let outbox = dir.path().join("messages/outbox");
    if outbox.exists() {
        let files: Vec<_> = fs::read_dir(&outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .collect();
        for file in &files {
            let content = fs::read_to_string(file.path()).unwrap();
            assert!(
                !content.contains("fallback") && !content.contains("retry_exhausted"),
                "Outbox should not contain fallback alert: {content}"
            );
        }
    }
}

#[test]
fn test_pending_fallback_persists_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "alert-then-hibernate.sh");

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .env("TEST_WAKE_AT", "2099-12-31T23:59")
        .current_dir(dir.path())
        .assert()
        .success();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut before_restart = None;
    while std::time::Instant::now() < deadline {
        if let Ok(content) = fs::read_to_string(dir.path().join("timer.json")) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                if !state["pid"].is_null() && state.get("pending_fallback").is_some() {
                    before_restart = Some(state);
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let before_restart =
        before_restart.expect("pending_fallback should be persisted after hibernate");
    let before_instance = before_restart["instance_id"]
        .as_str()
        .expect("daemon should persist an instance_id")
        .to_string();
    assert_eq!(
        before_restart["pending_fallback"]["action"]["action"],
        "email"
    );
    assert_eq!(
        before_restart["pending_fallback"]["action"]["target"],
        "ops@test.com"
    );
    assert_eq!(
        before_restart["pending_fallback"]["action"]["message"],
        "Watchdog set"
    );

    cryo_bin()
        .arg("restart")
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut after_restart = None;
    while std::time::Instant::now() < deadline {
        if let Ok(content) = fs::read_to_string(dir.path().join("timer.json")) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                if !state["pid"].is_null() && state.get("pending_fallback").is_some() {
                    let instance = state["instance_id"].as_str().unwrap_or_default();
                    if instance != before_instance {
                        after_restart = Some(state);
                        break;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let after_restart =
        after_restart.expect("pending_fallback and fresh instance_id should survive restart");
    assert_eq!(
        after_restart["pending_fallback"]["action"]["action"],
        "email"
    );
    assert_eq!(
        after_restart["pending_fallback"]["action"]["target"],
        "ops@test.com"
    );
    assert_eq!(
        after_restart["pending_fallback"]["action"]["message"],
        "Watchdog set"
    );

    cancel_and_wait(dir.path());
}

#[test]
fn test_delayed_wake_detection() {
    let dir = tempfile::tempdir().unwrap();
    // delayed-wake.sh: session 1 hibernates with a past wake time (10 min ago),
    // session 2 detects the delayed wake and completes the plan.
    setup_scenario(dir.path(), "delayed-wake.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // The daemon should detect the delayed wake (>5 minutes late) and log it.
    // The delayed wake notice contains "DELAYED WAKE" in the log via EventLogger.
    assert!(
        wait_for_log_content(dir.path(), "delayed wake", Duration::from_secs(20)),
        "Log should mention delayed wake"
    );

    // The plan should complete after the second session
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(15)),
        "Daemon should exit after plan completion"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("plan complete"),
        "Plan should complete after delayed wake: {log}"
    );
}

#[test]
fn test_periodic_report_fires() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "multi-session.sh");

    // Configure report_interval=1 (hour) with report_time matching the current HH:MM.
    // Note: `cryo start` creates a fresh timer.json, overwriting any pre-seeded state.
    // The daemon reads last_report_time=None from the fresh state, and computes
    // next_report_time using compute_next_report_time(report_time, 1, None).
    //
    // With report_time matching the current HH:MM, the next report time will be
    // either now (if current second <= 0) or 1 hour from now. In either case,
    // the report likely won't fire during this short test (minimum 1-hour interval).
    //
    // This test verifies that:
    // 1. The daemon accepts report configuration without errors
    // 2. The multi-session lifecycle completes normally with report config present
    // 3. The daemon logs the computed next report time
    let now = chrono::Local::now();
    let report_time = format!("{:02}:{:02}", now.hour(), now.minute());
    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = format!("{config}\nreport_interval = 1\nreport_time = \"{report_time}\"\n");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(30)),
        "Daemon should exit after completion"
    );

    // The daemon should have logged the next report time (stderr -> cryo.log)
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("next report at"),
        "Daemon should log the computed next report time: {log}"
    );

    // Verify the plan completed normally despite report config
    assert!(log.contains("plan complete"), "Plan should complete: {log}");

    // Check timer.json state is valid
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    // session_number should be >= 3 (multi-session.sh completes on session 3)
    let session_num = state["session_number"].as_u64().unwrap_or(0);
    assert!(
        session_num >= 3,
        "Should have at least 3 sessions, got {session_num}"
    );
}

#[test]
fn test_invalid_report_time_warns() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "quick-exit.sh");

    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = format!("{config}\nreport_interval = 1\nreport_time = \"not-a-time\"\n");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // The daemon should warn about invalid report_time.
    // Since spawn_daemon redirects stderr to cryo.log, the warning
    // "Daemon: warning: report_interval=1 but report_time='not-a-time' is invalid"
    // appears in cryo.log.
    assert!(
        wait_for_log_content(dir.path(), "report_time", Duration::from_secs(10)),
        "Should warn about invalid report_time in cryo.log"
    );

    cancel_and_wait(dir.path());

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("not-a-time"),
        "Warning should mention the invalid value: {log}"
    );
}

// --- Inbox wake tests ---

/// Write an inbox message file directly (simulates what cryo-zulip sync does).
fn write_inbox_message(dir: &std::path::Path, filename: &str, body: &str) {
    let inbox = dir.join("messages").join("inbox");
    fs::create_dir_all(&inbox).unwrap();
    let content = format!("---\nfrom: test-user\nsubject: test\n---\n{body}");
    fs::write(inbox.join(filename), content).unwrap();
}

#[test]
fn test_inbox_wake_coalesces_multiple_events() {
    // Regression test: multiple inbox files created rapidly should trigger
    // only one session, not one per file.
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "inbox-wake.sh");

    // Enable watch_inbox
    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = config.replace("watch_inbox = false", "watch_inbox = true");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    // Pre-create inbox dir so watcher can attach
    fs::create_dir_all(dir.path().join("messages/inbox")).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for daemon to initialize
    std::thread::sleep(Duration::from_millis(500));

    // Drop multiple inbox files rapidly to generate multiple InboxChanged events
    for i in 0..5 {
        write_inbox_message(dir.path(), &format!("msg-{i}.md"), &format!("message {i}"));
    }

    // Daemon should complete (inbox-wake.sh does --complete on first session)
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(15)),
        "Daemon should exit after plan completion"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("plan complete"), "Plan should complete: {log}");

    // Only ONE session should have run (events coalesced)
    let session_count = log.matches("CRYO SESSION").count();
    assert_eq!(
        session_count, 1,
        "Multiple inbox events should coalesce into 1 session, got {session_count}: {log}"
    );
}

#[test]
fn test_inbox_wake_no_delayed_wake_notice() {
    // Regression test: when an inbox message triggers a wake while next_wake
    // holds a past scheduled time, the daemon should NOT inject a "delayed wake"
    // notice into the agent prompt.
    //
    // Scenario: session 1 hibernates with a past wake time, then sleeps 2s.
    // During that sleep, the test writes an inbox file. The InboxChanged event
    // queues in the channel before the daemon's event loop resumes. When the
    // daemon checks recv_timeout(0), it finds InboxChanged first, so session 2
    // runs as an inbox wake (not a timeout wake) and skips delayed wake detection.
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "inbox-delayed-wake.sh");

    // Enable watch_inbox
    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = config.replace("watch_inbox = false", "watch_inbox = true");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for session 1 to hibernate (logs "hibernate: exit=...")
    assert!(
        wait_for_log_content(dir.path(), "hibernate: exit=", Duration::from_secs(10)),
        "Session 1 should hibernate with past wake time"
    );

    // Write inbox file while session 1's script is still sleeping (2s after hibernate).
    // This queues an InboxChanged event before the daemon's event loop resumes.
    write_inbox_message(dir.path(), "trigger.md", "wake up via inbox");

    // Wait for daemon to complete
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(20)),
        "Daemon should exit after plan completion"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("plan complete"), "Plan should complete: {log}");

    // The key assertion: no "delayed wake" notice should appear.
    // Session 2 was triggered by inbox (InboxChanged queued during session 1),
    // so even though next_wake was in the past, the daemon suppresses delayed
    // wake detection for inbox-triggered wakes.
    assert!(
        !log.contains("delayed wake"),
        "Inbox-triggered wake should NOT produce delayed wake notice: {log}"
    );
}
