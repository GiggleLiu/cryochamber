//! CLI edge case tests: user misuse, corrupted state, missing files.

use assert_cmd::Command;
use cryochamber::socket::{Request, Response, SocketServer};
use predicates::prelude::*;
use std::fs;
use std::sync::mpsc;
use std::time::Duration;

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
fn test_agent_hibernate_no_daemon() {
    let dir = tempfile::tempdir().unwrap();

    agent_bin()
        .args(["hibernate", "--complete"])
        .current_dir(dir.path())
        .assert()
        .failure(); // no socket -> connection error
}

#[test]
fn test_agent_receive_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    agent_bin()
        .args(["receive"])
        .current_dir(dir.path())
        .assert()
        .failure();
}

#[test]
fn test_agent_send_stdin_no_daemon_is_parsed() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());

    agent_bin()
        .args(["send", "--stdin"])
        .write_stdin("literal `cryo-agent dialog` text")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect to daemon socket"));
}

#[test]
fn test_agent_send_stdin_preserves_shell_sensitive_text() {
    let dir = tempfile::tempdir().unwrap();
    init_project(dir.path());
    let sock = cryochamber::socket::socket_path(dir.path());
    fs::create_dir_all(sock.parent().unwrap()).unwrap();

    let server = SocketServer::bind(&sock).unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let (request, responder) = server.accept_one(None).unwrap().unwrap();
            match request {
                Request::Hello { .. } => responder
                    .respond(&Response {
                        ok: true,
                        message: "IPC protocol ok".into(),
                    })
                    .unwrap(),
                Request::Send { text, question } => {
                    tx.send((text, question)).unwrap();
                    responder
                        .respond(&Response {
                            ok: true,
                            message: "Message sent".into(),
                        })
                        .unwrap();
                }
                other => panic!("unexpected request: {other:?}"),
            }
        }
    });

    agent_bin()
        .args(["send", "--stdin"])
        .write_stdin("literal `cryo-agent dialog` and $HOME\nsecond line\n")
        .current_dir(dir.path())
        .assert()
        .success();

    let (text, question) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(text, "literal `cryo-agent dialog` and $HOME\nsecond line\n");
    assert!(!question);
    handle.join().unwrap();
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

// Note: `cryo-agent receive` now goes through daemon IPC because archiving the
// inbox mutates chamber state. The low-level message/store tests still cover
// mailbox formatting and file operations directly.

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
