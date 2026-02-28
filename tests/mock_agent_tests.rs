//! Integration tests using mock agent scenario scripts.
//! Each test spawns a real daemon with CRYO_NO_SERVICE=1 and asserts on cryo.log/timer.json.

use std::fs;
use std::time::Duration;

/// Path to the cryo binary built by cargo.
fn cryo_bin() -> assert_cmd::Command {
    #[allow(deprecated)]
    assert_cmd::Command::cargo_bin("cryo").unwrap()
}

/// Initialize a cryo project in a temp directory with a specific scenario script.
fn setup_scenario(dir: &std::path::Path, scenario_name: &str) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let src = format!("{manifest}/tests/scenarios/{scenario_name}");
    fs::copy(&src, dir.join("scenario.sh")).unwrap();
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
fn wait_for_daemon_exit(dir: &std::path::Path, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(content) = fs::read_to_string(dir.join("timer.json")) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                if state["pid"].is_null() {
                    return true;
                }
            }
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

    // Verify all IPC commands were logged
    assert!(
        log.contains("note: \"Starting IPC test\""),
        "Missing note in log: {log}"
    );
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

    // Invalid wake time ("banana") is rejected by the daemon's NaiveDateTime parser.
    // The daemon sends an error response but the agent script has already exited,
    // so the daemon sees the agent exit without a successful hibernate call.
    assert!(
        wait_for_log_content(
            dir.path(),
            "agent exited without hibernate",
            Duration::from_secs(15)
        ),
        "Log should show agent exited without hibernate after invalid wake time"
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

    // The script sends: hibernate --wake "+5 seconds", then hibernate --complete.
    // "+5 seconds" is not valid ISO8601 (NaiveDateTime), so the daemon rejects
    // the first hibernate. The second hibernate --complete succeeds, so the
    // daemon completes the plan.
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
fn test_mock_note_after_hibernate() {
    let dir = tempfile::tempdir().unwrap();
    setup_scenario(dir.path(), "note-after-hibernate.sh");

    cryo_bin()
        .args(["start", "--agent", "mock", "--max-session-duration", "30"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Session should complete normally despite late note
    assert!(
        wait_for_daemon_exit(dir.path(), Duration::from_secs(15)),
        "Daemon should exit after plan completion"
    );

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("plan complete"),
        "Session should complete normally: {log}"
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

    // The script sends: hibernate --wake "+5 seconds", then exit 1.
    // "+5 seconds" is not valid ISO8601 (NaiveDateTime), so the daemon rejects
    // the hibernate. The agent then exits with code 1, so the daemon sees it as
    // "agent exited without hibernate" (a crash).
    assert!(
        wait_for_log_content(
            dir.path(),
            "agent exited without hibernate",
            Duration::from_secs(15)
        ),
        "Log should show agent exited without hibernate (wake time was invalid)"
    );

    cancel_and_wait(dir.path());
}
