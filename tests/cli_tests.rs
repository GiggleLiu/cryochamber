// tests/cli_tests.rs
// CLI integration tests using assert_cmd to cover main.rs command handlers.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn cmd() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo").unwrap()
}

// --- Init ---

#[test]
fn test_init_creates_protocol_and_plan() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTS.md"))
        .stdout(predicate::str::contains("plan.md"))
        .stdout(predicate::str::contains("messages/"));

    assert!(dir.path().join("AGENTS.md").exists());
    assert!(dir.path().join("plan.md").exists());
    assert!(dir.path().join("messages/inbox").is_dir());
    assert!(dir.path().join("messages/outbox").is_dir());
}

#[test]
fn test_init_claude_agent() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["init", "--agent", "claude"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("CLAUDE.md"));

    assert!(dir.path().join("CLAUDE.md").exists());
}

#[test]
fn test_init_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    // First init
    cmd()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Wrote"));

    // Second init — should say "already exists"
    cmd()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("already exists"));
}

// --- Status ---

#[test]
fn test_status_no_instance() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No cryochamber instance"));
}

#[test]
fn test_status_with_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = serde_json::json!({
        "plan_path": "plan.md",
        "session_number": 3,
        "last_command": "opencode",
        "pid": null,
        "max_retries": 1,
        "retry_count": 0
    });
    fs::write(
        dir.path().join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    cmd()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Plan: plan.md"))
        .stdout(predicate::str::contains("Session: 3"));
}

#[test]
fn test_status_shows_latest_session_tail() {
    let dir = tempfile::tempdir().unwrap();
    let state = serde_json::json!({
        "plan_path": "plan.md",
        "session_number": 1,
        "last_command": "opencode",
        "pid": null,
        "max_retries": 1,
        "retry_count": 0
    });
    fs::write(
        dir.path().join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    // Write a log file with a session
    let log_content = "--- CRYO SESSION 2026-02-23T10:00:00 ---\nSession: 1\nTask: test\n\nDid some work\n[CRYO:EXIT 0] All good\n--- CRYO END ---\n";
    fs::write(dir.path().join("cryo.log"), log_content).unwrap();

    cmd()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Latest session"))
        .stdout(predicate::str::contains("All good"));
}

// --- Log ---

#[test]
fn test_log_no_file() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("log")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No log file found"));
}

#[test]
fn test_log_with_content() {
    let dir = tempfile::tempdir().unwrap();
    let log_content = "--- CRYO SESSION 2026-02-23T10:00:00 ---\nSession: 1\nTask: test\n\nHello world\n--- CRYO END ---\n";
    fs::write(dir.path().join("cryo.log"), log_content).unwrap();

    cmd()
        .arg("log")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello world"))
        .stdout(predicate::str::contains("CRYO SESSION"));
}

// --- Validate ---

#[test]
fn test_validate_no_log() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .failure();
}

#[test]
fn test_validate_plan_complete() {
    let dir = tempfile::tempdir().unwrap();
    let log_content = "--- CRYO SESSION 2026-02-23T10:00:00 ---\nSession: 1\nTask: test\n\n[CRYO:EXIT 0] All done\n--- CRYO END ---\n";
    fs::write(dir.path().join("cryo.log"), log_content).unwrap();

    cmd()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Plan is complete"));
}

#[test]
fn test_validate_can_hibernate() {
    let dir = tempfile::tempdir().unwrap();
    let log_content = "--- CRYO SESSION 2026-02-23T10:00:00 ---\nSession: 1\nTask: test\n\n[CRYO:EXIT 0] Partial\n[CRYO:WAKE 2099-12-31T23:59]\n--- CRYO END ---\n";
    fs::write(dir.path().join("cryo.log"), log_content).unwrap();

    cmd()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("All checks passed"));
}

#[test]
fn test_validate_missing_exit_marker() {
    let dir = tempfile::tempdir().unwrap();
    let log_content = "--- CRYO SESSION 2026-02-23T10:00:00 ---\nSession: 1\nTask: test\n\nNo markers here\n--- CRYO END ---\n";
    fs::write(dir.path().join("cryo.log"), log_content).unwrap();

    cmd()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("ERROR"))
        .stdout(predicate::str::contains("Validation FAILED"));
}

// --- Cancel ---

#[test]
fn test_cancel_no_instance() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No cryochamber instance"));
}

// --- Start ---

#[test]
fn test_start_nonexistent_plan() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["start", "nonexistent.md"])
        .current_dir(dir.path())
        .assert()
        .failure();
}

// --- Help ---

#[test]
fn test_help() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Long-term AI agent task scheduler",
        ));
}

#[test]
fn test_no_subcommand() {
    cmd()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

// --- Mock agent helpers ---

/// Path to the mock agent script relative to the project root.
fn mock_agent_cmd() -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    format!("{manifest}/tests/mock_agent.sh")
}

/// Path to the cryo binary built by cargo.
fn cryo_bin_path() -> String {
    #[allow(deprecated)]
    let path = assert_cmd::cargo::cargo_bin("cryo");
    path.to_string_lossy().to_string()
}

// --- Tests using mock agent ---

#[test]
fn test_fallback_exec_writes_outbox() {
    let dir = tempfile::tempdir().unwrap();
    // Ensure message dirs exist
    fs::create_dir_all(dir.path().join("messages/outbox")).unwrap();
    fs::create_dir_all(dir.path().join("messages/inbox")).unwrap();

    cmd()
        .args([
            "fallback-exec",
            "email",
            "user@example.com",
            "Task failed after 3 retries",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Fallback alert written"));

    // Check outbox has a file
    let outbox = fs::read_dir(dir.path().join("messages/outbox")).unwrap();
    let files: Vec<_> = outbox.collect();
    assert_eq!(files.len(), 1);

    let content = fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
    assert!(content.contains("Task failed after 3 retries"));
    assert!(content.contains("email"));
}

// --- Send ---

#[test]
fn test_send_creates_inbox_message() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["send", "e2e4"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check that a file was created in messages/inbox/
    let inbox = dir.path().join("messages").join("inbox");
    let entries: Vec<_> = std::fs::read_dir(&inbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert_eq!(entries.len(), 1);

    let content = std::fs::read_to_string(entries[0].path()).unwrap();
    assert!(content.contains("from: human"));
    assert!(content.contains("e2e4"));
}

#[test]
fn test_send_with_subject_and_from() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args([
            "send",
            "--subject",
            "chess move",
            "--from",
            "player1",
            "e2e4",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let inbox = dir.path().join("messages").join("inbox");
    let entries: Vec<_> = std::fs::read_dir(&inbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert_eq!(entries.len(), 1);

    let content = std::fs::read_to_string(entries[0].path()).unwrap();
    assert!(content.contains("from: player1"));
    assert!(content.contains("subject: chess move"));
}

#[test]
fn test_send_no_body_fails() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["send"])
        .current_dir(dir.path())
        .assert()
        .failure();
}

// --- Receive ---

#[test]
fn test_receive_empty_outbox() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["receive"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("No messages"));
}

#[test]
fn test_receive_shows_outbox_messages() {
    let dir = tempfile::tempdir().unwrap();
    // Write a message to outbox manually
    cryochamber::message::ensure_dirs(dir.path()).unwrap();
    let msg = cryochamber::message::Message {
        from: "cryochamber".to_string(),
        subject: "Board update".to_string(),
        body: "AI played Nf3".to_string(),
        timestamp: chrono::NaiveDateTime::parse_from_str(
            "2026-02-23T10:00:00",
            "%Y-%m-%dT%H:%M:%S",
        )
        .unwrap(),
        metadata: std::collections::BTreeMap::new(),
    };
    cryochamber::message::write_message(dir.path(), "outbox", &msg).unwrap();

    cmd()
        .args(["receive"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("Board update"))
        .stdout(predicates::str::contains("AI played Nf3"));
}

// --- Backward compat ---

#[test]
fn test_state_backward_compat_without_daemon_fields() {
    let dir = tempfile::tempdir().unwrap();
    // Old-format state without daemon fields
    let state = serde_json::json!({
        "plan_path": "plan.md",
        "session_number": 1,
        "last_command": "opencode",
        "pid": null,
        "max_retries": 1,
        "retry_count": 0
    });
    fs::write(
        dir.path().join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    // Should load without error, daemon fields get defaults
    cmd()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Session: 1"));
}

// --- Daemon tests ---

#[test]
fn test_daemon_plan_complete() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan\nDo stuff").unwrap();

    // Start with daemon mode (default)
    // CRYO_BIN tells the mock agent to call `cryo hibernate --complete` via socket
    cmd()
        .args(["start", "plan.md", "--agent", &mock_agent_cmd()])
        .env("MOCK_AGENT_OUTPUT", "[CRYO:EXIT 0] All done")
        .env("CRYO_BIN", cryo_bin_path())
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Cryochamber started"));

    // Wait for daemon to finish (it should exit after plan complete)
    // Poll for up to 10 seconds
    let mut daemon_exited = false;
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(content) = fs::read_to_string(dir.path().join("timer.json")) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                if state["pid"].is_null() {
                    daemon_exited = true;
                    break;
                }
            }
        }
    }
    assert!(daemon_exited, "Daemon should have exited within 10 seconds");

    // Check state: PID should be cleared, daemon_mode false
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    assert!(state["pid"].is_null());
    assert_eq!(state["daemon_mode"].as_bool(), Some(false));

    // Check log contains session event (EventLogger writes events, not agent stdout)
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("plan complete"));
}

#[test]
fn test_daemon_cancel() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan").unwrap();

    // Use a slow agent that sleeps
    let agent = "/bin/sh -c 'sleep 30 && echo [CRYO:EXIT 0] Done'";

    cmd()
        .args(["start", "plan.md", "--agent", agent])
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for daemon to start
    std::thread::sleep(std::time::Duration::from_secs(1));

    // Cancel should work
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Cryochamber cancelled"));
}

#[test]
fn test_daemon_inbox_reactive_wake() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan").unwrap();

    // Start with daemon mode, verify state has watch_inbox: true
    // CRYO_BIN tells the mock agent to call `cryo hibernate --complete` via socket
    cmd()
        .args(["start", "plan.md", "--agent", &mock_agent_cmd()])
        .env("MOCK_AGENT_OUTPUT", "[CRYO:EXIT 0] Done")
        .env("CRYO_BIN", cryo_bin_path())
        .current_dir(dir.path())
        .assert()
        .success();

    std::thread::sleep(std::time::Duration::from_secs(2));

    // State should have watch_inbox: true
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    assert_eq!(state["watch_inbox"].as_bool(), Some(true));
}

#[test]
fn test_daemon_status_shows_daemon_mode() {
    let dir = tempfile::tempdir().unwrap();
    let state = serde_json::json!({
        "plan_path": "plan.md",
        "session_number": 1,
        "last_command": "opencode",
        "pid": null,
        "max_retries": 1,
        "retry_count": 0,
        "max_session_duration": 1800,
        "watch_inbox": true,
        "daemon_mode": true
    });
    fs::write(
        dir.path().join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    cmd()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Daemon mode: yes"))
        .stdout(predicate::str::contains("Session timeout: 1800s"));
}

#[test]
fn test_session_logs_inbox_filenames() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan\nPlay chess").unwrap();

    // Send a message before starting
    cmd()
        .args(["send", "e2e4"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Run a session with mock agent via daemon
    // CRYO_BIN tells the mock agent to call `cryo hibernate --complete` via socket
    cmd()
        .args(["start", "plan.md", "--agent", &mock_agent_cmd()])
        .env("MOCK_AGENT_OUTPUT", "[CRYO:EXIT 0] Done")
        .env("CRYO_BIN", cryo_bin_path())
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for daemon to finish
    let mut daemon_exited = false;
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(content) = fs::read_to_string(dir.path().join("timer.json")) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                if state["pid"].is_null() {
                    daemon_exited = true;
                    break;
                }
            }
        }
    }
    assert!(daemon_exited, "Daemon should have exited within 10 seconds");

    // Check cryo.log contains inbox line (EventLogger format: "inbox: N messages (file1, ...)")
    let log_content = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log_content.contains("inbox: 1 messages"));
}

// --- Hibernate / Note / Reply ---

#[test]
fn test_hibernate_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["hibernate", "--wake", "2099-01-01T00:00"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_note_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["note", "test note"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_reply_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["reply", "hello human"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_hibernate_complete_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["hibernate", "--complete"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_hibernate_requires_wake_or_complete() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .args(["hibernate"])
        .current_dir(dir.path())
        .assert()
        .failure();
}
