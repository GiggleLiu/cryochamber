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
        "wake_timer_id": "com.cryochamber.abc.wake",
        "fallback_timer_id": null,
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
        .stdout(predicate::str::contains("Session: 3"))
        .stdout(predicate::str::contains("com.cryochamber.abc.wake"));
}

#[test]
fn test_status_shows_latest_session_tail() {
    let dir = tempfile::tempdir().unwrap();
    let state = serde_json::json!({
        "plan_path": "plan.md",
        "session_number": 1,
        "last_command": "opencode",
        "wake_timer_id": null,
        "fallback_timer_id": null,
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

// --- Wake ---

#[test]
fn test_wake_no_state() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("wake")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No cryochamber state found"));
}

#[test]
fn test_wake_no_plan() {
    let dir = tempfile::tempdir().unwrap();
    let state = serde_json::json!({
        "plan_path": "plan.md",
        "session_number": 1,
        "last_command": "echo",
        "wake_timer_id": null,
        "fallback_timer_id": null,
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
        .arg("wake")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("plan.md not found"));
}

// --- Start ---

#[test]
fn test_start_no_markers_agent() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Test Plan\nDo stuff").unwrap();

    // Use true as a fake agent — produces no output, no markers.
    // Validation will fail, but the session mechanics still execute.
    cmd()
        .args(["start", "plan.md", "--agent", "true", "--foreground"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Pre-hibernate validation failed"));

    // State and log should have been created
    assert!(dir.path().join("timer.json").exists());
    assert!(dir.path().join("cryo.log").exists());
}

#[test]
fn test_start_plan_complete_agent() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Test Plan\nDo stuff").unwrap();

    // Use /bin/sh -c to simulate an agent that emits EXIT without WAKE (= plan complete).
    // sh -c takes the next arg as the script; the --prompt arg becomes $0 and is ignored.
    cmd()
        .args([
            "start",
            "plan.md",
            "--agent",
            "/bin/sh -c 'echo [CRYO:EXIT 0] All done'",
            "--foreground",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Plan complete"));

    assert!(dir.path().join("timer.json").exists());
    assert!(dir.path().join("cryo.log").exists());
}

#[test]
fn test_start_copies_plan_to_workdir() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("plans");
    fs::create_dir_all(&subdir).unwrap();
    fs::write(subdir.join("my-plan.md"), "# My Plan").unwrap();

    cmd()
        .args([
            "start",
            &subdir.join("my-plan.md").to_string_lossy(),
            "--agent",
            "/bin/sh -c 'echo [CRYO:EXIT 0] Done'",
            "--foreground",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // plan.md should be a copy in the working directory
    assert!(dir.path().join("plan.md").exists());
    let content = fs::read_to_string(dir.path().join("plan.md")).unwrap();
    assert_eq!(content, "# My Plan");
}

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

// --- Tests using mock agent ---

#[test]
fn test_start_mock_agent_plan_complete() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan\nDo it").unwrap();

    cmd()
        .args([
            "start",
            "plan.md",
            "--agent",
            &mock_agent_cmd(),
            "--foreground",
        ])
        .env("MOCK_AGENT_OUTPUT", "[CRYO:EXIT 0] All tasks done")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Plan complete"));
}

#[test]
fn test_start_mock_agent_partial_exit() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan").unwrap();

    // EXIT 1 (partial) with no WAKE → plan complete
    cmd()
        .args([
            "start",
            "plan.md",
            "--agent",
            &mock_agent_cmd(),
            "--foreground",
        ])
        .env("MOCK_AGENT_OUTPUT", "[CRYO:EXIT 1] Partial progress")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Plan complete"));

    // Verify the log captured the agent output
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("Partial progress"));
}

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

    // Run a session with mock agent (will read the inbox message)
    cmd()
        .args([
            "start",
            "plan.md",
            "--agent",
            &mock_agent_cmd(),
            "--foreground",
        ])
        .env("MOCK_AGENT_OUTPUT", "[CRYO:EXIT 0] Done")
        .current_dir(dir.path())
        .assert()
        .success();

    // Check cryo.log contains [inbox] line
    let log_content = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log_content.contains("[inbox]"));
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
        "wake_timer_id": null,
        "fallback_timer_id": null,
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

// --- Full wake cycle (macOS only — requires real launchd) ---

#[cfg(target_os = "macos")]
#[test]
fn test_start_wake_cycle_with_timer() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan\nMulti-session").unwrap();

    // Agent outputs EXIT + WAKE (far future) + CMD
    let output =
        "[CRYO:EXIT 0] Session done\n[CRYO:WAKE 2099-12-31T23:59]\n[CRYO:CMD echo continue]";
    cmd()
        .args([
            "start",
            "plan.md",
            "--agent",
            &mock_agent_cmd(),
            "--foreground",
        ])
        .env("MOCK_AGENT_OUTPUT", output)
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hibernating"))
        .stdout(predicate::str::contains("2099-12-31"));

    // Verify state has timer IDs
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    assert!(state["wake_timer_id"].is_string());

    // Clean up: cancel the timer we just registered
    let wake_id = state["wake_timer_id"].as_str().unwrap();
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(wake_id));
}

#[cfg(target_os = "macos")]
#[test]
fn test_start_with_fallback_timer() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan").unwrap();

    let output = "[CRYO:EXIT 0] Done\n[CRYO:WAKE 2099-06-15T10:00]\n[CRYO:FALLBACK email admin@co.com \"agent stuck\"]";
    cmd()
        .args([
            "start",
            "plan.md",
            "--agent",
            &mock_agent_cmd(),
            "--foreground",
        ])
        .env("MOCK_AGENT_OUTPUT", output)
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hibernating"));

    // State should have both wake and fallback timer IDs
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    assert!(state["wake_timer_id"].is_string());
    assert!(state["fallback_timer_id"].is_string());

    // Clean up both timers
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .success();
}
