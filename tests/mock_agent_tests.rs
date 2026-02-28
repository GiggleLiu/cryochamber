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

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("agent exited without hibernate"),
        "Log should show agent exited without hibernate: {log}"
    );
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

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("quick exit detected"),
        "Log should detect quick exit: {log}"
    );
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

    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("session timeout"),
        "Log should show session timeout: {log}"
    );
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
