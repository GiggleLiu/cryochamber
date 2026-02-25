// tests/cli_tests.rs
// CLI integration tests using assert_cmd to cover binary command handlers.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn cmd() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo").unwrap()
}

fn agent_cmd() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo-agent").unwrap()
}

/// Run `cryo init` in a temp dir so tests that need `cryo start` have protocol files.
fn init_dir(dir: &std::path::Path) {
    cmd().arg("init").current_dir(dir).assert().success();
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
        .stdout(predicate::str::contains("cryo.toml"))
        .stdout(predicate::str::contains("AGENTS.md"))
        .stdout(predicate::str::contains("plan.md"));

    assert!(dir.path().join("cryo.toml").exists());
    assert!(dir.path().join("AGENTS.md").exists());
    assert!(dir.path().join("plan.md").exists());
    assert!(dir.path().join("messages/inbox").is_dir());
    assert!(dir.path().join("messages/outbox").is_dir());

    // Verify cryo.toml contains the default agent
    let config_content = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    assert!(config_content.contains("agent = \"opencode\""));
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

    // Verify cryo.toml has claude as agent
    let config_content = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    assert!(config_content.contains("agent = \"claude\""));
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
        .stdout(predicate::str::contains("created"));

    // Second init — should say "exists, kept" on stdout
    cmd()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("exists, kept"));
}

// --- Status ---

#[test]
fn test_status_no_instance() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No cryochamber project"));
}

#[test]
fn test_status_with_state() {
    let dir = tempfile::tempdir().unwrap();
    init_dir(dir.path());
    let state = serde_json::json!({
        "session_number": 3,
        "pid": null,
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
        .stdout(predicate::str::contains("Daemon: stopped"))
        .stdout(predicate::str::contains("Session: 3"))
        .stdout(predicate::str::contains("Agent: opencode"));
}

#[test]
fn test_status_shows_latest_session_tail() {
    let dir = tempfile::tempdir().unwrap();
    init_dir(dir.path());
    let state = serde_json::json!({
        "session_number": 1,
        "pid": null,
        "retry_count": 0
    });
    fs::write(
        dir.path().join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    // Write a log file with a session (new EventLogger format)
    let log_content = "--- CRYO SESSION 1 | 2026-02-23T10:00:00Z ---\ntask: test\nagent: opencode\ninbox: 0 messages\n[10:00:01] agent started (pid 12345)\n[10:00:05] hibernate: plan complete, exit=0, summary=\"All good\"\n[10:00:05] agent exited (code 0)\n[10:00:05] session complete\n--- CRYO END ---\n";
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

// --- Cancel ---

#[test]
fn test_cancel_no_instance() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No cryochamber project"));
}

#[test]
fn test_cancel_stale_state() {
    let dir = tempfile::tempdir().unwrap();
    init_dir(dir.path());
    // Write stale state with a dead PID (pid=1 is init, won't match our process)
    let state = serde_json::json!({
        "session_number": 2,
        "pid": 999999,
        "retry_count": 0
    });
    fs::write(
        dir.path().join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    // Cancel should succeed (clean up stale state) instead of failing
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed timer.json"));

    // timer.json should be gone
    assert!(!dir.path().join("timer.json").exists());
}

#[test]
fn test_cancel_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    init_dir(dir.path());
    // No timer.json at all
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Nothing to cancel"));
}

// --- Start ---

#[test]
fn test_start_no_plan_md() {
    let dir = tempfile::tempdir().unwrap();
    // Init without plan.md — remove the auto-created one
    init_dir(dir.path());
    fs::remove_file(dir.path().join("plan.md")).unwrap();
    cmd()
        .arg("start")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No plan.md found"));
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

/// Path to the cryo-agent binary built by cargo.
fn cryo_agent_bin_path() -> String {
    #[allow(deprecated)]
    let path = assert_cmd::cargo::cargo_bin("cryo-agent");
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
    init_dir(dir.path());
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
    init_dir(dir.path());
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
fn test_state_backward_compat_ignores_unknown_fields() {
    // Old-format state with fields that no longer exist — serde ignores them
    let dir = tempfile::tempdir().unwrap();
    init_dir(dir.path());
    let state = serde_json::json!({
        "plan_path": "plan.md",
        "session_number": 1,
        "last_command": "opencode",
        "pid": null,
        "max_retries": 1,
        "retry_count": 0,
        "max_session_duration": 1800,
        "watch_inbox": true,
        "daemon_mode": false
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
        .stdout(predicate::str::contains("Session: 1"));
}

// --- Daemon tests ---

#[test]
fn test_daemon_plan_complete() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan\nDo stuff").unwrap();
    init_dir(dir.path());

    // Start with daemon mode (default)
    // CRYO_AGENT_BIN tells the mock agent to call `cryo-agent hibernate --complete` via socket
    cmd()
        .args(["start", "--agent", &mock_agent_cmd()])
        .env("CRYO_AGENT_BIN", cryo_agent_bin_path())
        .env("CRYO_NO_SERVICE", "1")
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

    // Check state: PID should be cleared
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    assert!(state["pid"].is_null());

    // Check log contains session event (EventLogger writes events, not agent stdout)
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("plan complete"));
}

#[test]
fn test_daemon_cancel() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan").unwrap();
    init_dir(dir.path());

    // Use a slow agent that sleeps (doesn't need to hibernate, test just cancels it)
    let agent = "/bin/sh -c 'sleep 30'";

    cmd()
        .args(["start", "--agent", agent])
        .env("CRYO_NO_SERVICE", "1")
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
fn test_wake_signal_wakes_daemon() {
    // Daemon with watch_inbox=false should still respond to `cryo wake` (SIGUSR1).
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan").unwrap();
    init_dir(dir.path());

    // Disable watch_inbox
    let config = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let config = config.replace("watch_inbox = true", "watch_inbox = false");
    fs::write(dir.path().join("cryo.toml"), config).unwrap();

    // Start daemon with a mock agent that hibernates with a far-future wake (not --complete)
    let agent = &mock_agent_cmd();
    cmd()
        .args(["start", "--agent", agent])
        .env("CRYO_AGENT_BIN", cryo_agent_bin_path())
        .env("CRYO_NO_SERVICE", "1")
        .env("MOCK_AGENT_COMPLETE", "false")
        .env("MOCK_AGENT_WAKE", "2099-12-31T23:59")
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait for first session to complete and daemon to sleep
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify daemon is alive
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    assert!(state_content.contains("\"pid\""));

    // `cryo wake` should succeed and signal the daemon
    cmd()
        .arg("wake")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Wake signal sent"));

    // Wait for daemon to wake and run the new session
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify a second session was logged (daemon woke and ran)
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("CRYO SESSION 2"),
        "Expected session 2 after wake signal, got:\n{log}"
    );

    // Cleanup
    cmd()
        .arg("cancel")
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_daemon_config_watch_inbox() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan").unwrap();
    init_dir(dir.path());

    // Verify cryo.toml has watch_inbox: true (default)
    let config_content = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    assert!(config_content.contains("watch_inbox = true"));

    // Start with daemon mode
    // CRYO_AGENT_BIN tells the mock agent to call `cryo-agent hibernate --complete` via socket
    cmd()
        .args(["start", "--agent", &mock_agent_cmd()])
        .env("CRYO_AGENT_BIN", cryo_agent_bin_path())
        .env("CRYO_NO_SERVICE", "1")
        .current_dir(dir.path())
        .assert()
        .success();

    std::thread::sleep(std::time::Duration::from_secs(2));

    // timer.json should be slim — no watch_inbox field at all
    let state_content = fs::read_to_string(dir.path().join("timer.json")).unwrap();
    assert!(!state_content.contains("watch_inbox"));
}

#[test]
fn test_daemon_status_shows_config() {
    let dir = tempfile::tempdir().unwrap();
    init_dir(dir.path());

    // Update cryo.toml to set max_session_duration
    let config_content = fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    let updated = config_content.replace("max_session_duration = 0", "max_session_duration = 1800");
    fs::write(dir.path().join("cryo.toml"), updated).unwrap();

    let state = serde_json::json!({
        "session_number": 1,
        "pid": null,
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
        .stdout(predicate::str::contains("Daemon: stopped"))
        .stdout(predicate::str::contains("Session timeout: 1800s"));
}

#[test]
fn test_session_logs_inbox_filenames() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plan.md"), "# Plan\nPlay chess").unwrap();
    init_dir(dir.path());

    // Send a message before starting
    cmd()
        .args(["send", "e2e4"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Run a session with mock agent via daemon
    // CRYO_AGENT_BIN tells the mock agent to call `cryo-agent hibernate --complete` via socket
    cmd()
        .args(["start", "--agent", &mock_agent_cmd()])
        .env("CRYO_AGENT_BIN", cryo_agent_bin_path())
        .env("CRYO_NO_SERVICE", "1")
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

// --- cryo-agent binary tests ---

#[test]
fn test_agent_hibernate_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["hibernate", "--wake", "2099-01-01T00:00"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_agent_note_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["note", "test note"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_agent_hibernate_requires_wake_or_complete() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["hibernate"])
        .current_dir(dir.path())
        .assert()
        .failure();
}
