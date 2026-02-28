//! CLI edge case tests: user misuse, corrupted state, missing files.

use assert_cmd::Command;
use std::fs;

fn cryo_bin() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo").unwrap()
}

fn agent_bin() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo-agent").unwrap()
}

/// Initialize a minimal cryo project.
fn init_project(dir: &std::path::Path) {
    fs::write(dir.join("plan.md"), "# Test Plan\nDo things.").unwrap();
    cryo_bin()
        .args(["init", "--agent", "mock"])
        .current_dir(dir)
        .assert()
        .success();
}

// --- Commands against stopped daemon ---

#[test]
fn test_status_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    cryo_bin()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success(); // status should not crash even if daemon not running
}

#[test]
fn test_cancel_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    // Cancel with no timer.json should fail with "Nothing to cancel"
    cryo_bin()
        .args(["cancel"])
        .current_dir(dir.path())
        .assert()
        .failure();
}

#[test]
fn test_wake_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    // wake writes an inbox message and warns that no daemon is running,
    // but still exits successfully (the message is queued)
    cryo_bin()
        .args(["wake"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_send_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    // Send should write to inbox even without daemon
    cryo_bin()
        .args(["send", "Hello from test"])
        .current_dir(dir.path())
        .assert()
        .success();

    let inbox = dir.path().join("messages/inbox");
    assert!(inbox.exists(), "Inbox directory should exist after init");
    let files: Vec<_> = fs::read_dir(&inbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert!(!files.is_empty(), "Inbox should have the sent message");
}

#[test]
fn test_agent_note_no_daemon() {
    let dir = tempfile::tempdir().unwrap();

    agent_bin()
        .args(["note", "test note"])
        .current_dir(dir.path())
        .assert()
        .failure(); // no socket -> connection error
}

#[test]
fn test_agent_hibernate_no_daemon() {
    let dir = tempfile::tempdir().unwrap();

    agent_bin()
        .args(["hibernate", "--complete"])
        .current_dir(dir.path())
        .assert()
        .failure(); // no socket -> connection error
}

// --- Double start / stale lock ---

#[test]
fn test_start_while_running() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    // Start first daemon (use sleep as a long-running agent that won't exit)
    cryo_bin()
        .args(["start", "--agent", "/bin/sh -c 'sleep 30'"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for daemon to be running
    std::thread::sleep(std::time::Duration::from_secs(1));

    // Try to start again — should fail with "already running"
    cryo_bin()
        .args(["start", "--agent", "/bin/sh -c 'sleep 30'"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .failure();

    // Clean up
    let _ = cryo_bin().args(["cancel"]).current_dir(dir.path()).output();
    std::thread::sleep(std::time::Duration::from_secs(1));
}

#[test]
fn test_start_stale_pid_lock() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    // Spawn a process that exits immediately to get a dead PID
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = child.id();
    child.wait().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let state = serde_json::json!({
        "session_number": 1,
        "pid": dead_pid,
        "retry_count": 0
    });
    fs::write(
        dir.path().join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    // Start should succeed — stale lock overridden (is_locked returns false for dead PID)
    cryo_bin()
        .args(["start", "--agent", "/bin/sh -c 'sleep 30'"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    std::thread::sleep(std::time::Duration::from_secs(1));
    let _ = cryo_bin().args(["cancel"]).current_dir(dir.path()).output();
    std::thread::sleep(std::time::Duration::from_secs(1));
}

// --- Corrupted project state ---

#[test]
fn test_start_missing_plan() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());
    fs::remove_file(dir.path().join("plan.md")).unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .failure();
}

#[test]
fn test_start_corrupted_config() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());
    fs::write(dir.path().join("cryo.toml"), "{{{{ garbage").unwrap();

    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .failure();
}

#[test]
fn test_start_corrupted_state() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());
    fs::write(dir.path().join("timer.json"), "{broken").unwrap();

    // Corrupted JSON in timer.json causes load_state to return Err,
    // which propagates as a failure from cmd_start
    cryo_bin()
        .args(["start", "--agent", "mock"])
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .failure();
}

// --- Message edge cases ---

#[test]
fn test_send_creates_inbox_directory() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    // Remove messages directory if it exists
    let messages_dir = dir.path().join("messages");
    if messages_dir.exists() {
        fs::remove_dir_all(&messages_dir).unwrap();
    }

    cryo_bin()
        .args(["send", "Hello"])
        .current_dir(dir.path())
        .assert()
        .success();

    assert!(
        dir.path().join("messages/inbox").exists(),
        "Inbox directory should be created"
    );
}

#[test]
fn test_receive_empty_inbox() {
    let dir = tempfile::tempdir().unwrap();

    agent_bin()
        .args(["receive"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_receive_malformed_message() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages/inbox");
    fs::create_dir_all(&inbox).unwrap();
    fs::write(inbox.join("bad-message.md"), "not valid frontmatter {{{").unwrap();

    agent_bin()
        .args(["receive"])
        .current_dir(dir.path())
        .assert()
        .success(); // should not crash on malformed messages
}

// --- Time subcommand ---

#[test]
fn test_time_no_offset() {
    agent_bin()
        .args(["time"])
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}").unwrap());
}

#[test]
fn test_time_invalid_offset() {
    agent_bin().args(["time", "+3 bananas"]).assert().failure();
}
