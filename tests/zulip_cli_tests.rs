//! CLI integration tests for the `cryo-zulip` binary.
//!
//! These tests exercise command routing, file-based state loading and the
//! "happy path" early-exit branches (no session log, already pushed, etc.)
//! without touching the Zulip API.

use assert_cmd::Command;
use std::fs;

fn zulip_bin() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo-zulip").unwrap()
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Write a minimal `.cryo/zuliprc` so `from_zuliprc` can parse it.
fn write_fake_zuliprc(dir: &std::path::Path) {
    let cryo_dir = dir.join(".cryo");
    fs::create_dir_all(&cryo_dir).unwrap();
    fs::write(
        cryo_dir.join("zuliprc"),
        "[api]\nemail=bot@example.com\nkey=fakekey\nsite=https://zulip.example.com\n",
    )
    .unwrap();
}

/// Write a minimal `zulip-sync.json` pointing at a fake stream.
fn write_zulip_sync(dir: &std::path::Path, last_pushed: Option<u32>) {
    let state = serde_json::json!({
        "site": "https://zulip.example.com",
        "stream": "dev",
        "stream_id": 1,
        "self_email": "bot@example.com",
        "last_pushed_session": last_pushed
    });
    fs::write(
        dir.join("zulip-sync.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Write a minimal `timer.json` with the given session number.
fn write_timer(dir: &std::path::Path, session_number: u32) {
    let state = serde_json::json!({"session_number": session_number});
    fs::write(
        dir.join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Write a minimal `cryo.log` containing one completed session so
/// `read_latest_session` returns `Some(...)`.
fn write_minimal_log(dir: &std::path::Path) {
    fs::write(
        dir.join("cryo.log"),
        "--- CRYO SESSION 1 ---\ntask: test\n--- CRYO END ---\n",
    )
    .unwrap();
}

// ── status ────────────────────────────────────────────────────────────────────

#[test]
fn test_zulip_status_not_configured() {
    let dir = tempfile::tempdir().unwrap();

    zulip_bin()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("not configured"));
}

#[test]
fn test_zulip_status_configured() {
    let dir = tempfile::tempdir().unwrap();
    write_zulip_sync(dir.path(), None);

    zulip_bin()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("zulip.example.com"))
        .stdout(predicates::str::contains("dev"))
        .stdout(predicates::str::contains("bot@example.com"));
}

#[test]
fn test_zulip_status_shows_last_pushed_session() {
    let dir = tempfile::tempdir().unwrap();
    write_zulip_sync(dir.path(), Some(7));

    zulip_bin()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("7"));
}

// ── pull ──────────────────────────────────────────────────────────────────────

#[test]
fn test_zulip_pull_without_sync_json_fails() {
    let dir = tempfile::tempdir().unwrap();

    zulip_bin()
        .args(["pull"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("zulip-sync.json"));
}

// ── push ──────────────────────────────────────────────────────────────────────

#[test]
fn test_zulip_push_without_sync_json_fails() {
    let dir = tempfile::tempdir().unwrap();

    zulip_bin()
        .args(["push"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("zulip-sync.json"));
}

#[test]
fn test_zulip_push_no_session_log() {
    let dir = tempfile::tempdir().unwrap();
    write_zulip_sync(dir.path(), None);
    write_fake_zuliprc(dir.path());
    // No cryo.log — should exit early with a friendly message

    zulip_bin()
        .args(["push"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("No session log found"));
}

#[test]
fn test_zulip_push_already_pushed_session() {
    let dir = tempfile::tempdir().unwrap();
    write_zulip_sync(dir.path(), Some(1));
    write_fake_zuliprc(dir.path());
    write_minimal_log(dir.path());
    write_timer(dir.path(), 1);
    // session_num (1) == last_pushed_session (Some(1)) → should skip

    zulip_bin()
        .args(["push"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("already pushed"));
}

// ── unsync without service ────────────────────────────────────────────────────

#[test]
fn test_zulip_unsync_no_service() {
    let dir = tempfile::tempdir().unwrap();

    zulip_bin()
        .args(["unsync"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("No sync service"));
}

