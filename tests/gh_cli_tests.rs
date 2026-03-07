//! CLI integration tests for the `cryo-gh` binary.
//!
//! Tests exercise command routing, file-based state loading and early-exit
//! branches (no session log, not configured, etc.) without touching the
//! GitHub API.

use assert_cmd::Command;
use std::fs;

fn gh_bin() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo-gh").unwrap()
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Write a minimal `gh-sync.json`.
fn write_gh_sync(dir: &std::path::Path, last_pushed: Option<u32>) {
    let state = serde_json::json!({
        "repo": "octocat/hello-world",
        "discussion_number": 42,
        "discussion_node_id": "D_node123",
        "last_pushed_session": last_pushed
    });
    fs::write(
        dir.join("gh-sync.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Write a minimal `cryo.log` containing one completed session.
fn write_minimal_log(dir: &std::path::Path) {
    fs::write(
        dir.join("cryo.log"),
        "--- CRYO SESSION 1 ---\ntask: test\n--- CRYO END ---\n",
    )
    .unwrap();
}

/// Write a minimal `timer.json`.
fn write_timer(dir: &std::path::Path, session_number: u32) {
    let state = serde_json::json!({"session_number": session_number});
    fs::write(
        dir.join("timer.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

// ── status ────────────────────────────────────────────────────────────────────

#[test]
fn test_gh_status_not_configured() {
    let dir = tempfile::tempdir().unwrap();

    gh_bin()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("not configured"));
}

#[test]
fn test_gh_status_configured() {
    let dir = tempfile::tempdir().unwrap();
    write_gh_sync(dir.path(), None);

    gh_bin()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("octocat/hello-world"))
        .stdout(predicates::str::contains("42"));
}

#[test]
fn test_gh_status_shows_last_pushed_session() {
    let dir = tempfile::tempdir().unwrap();
    write_gh_sync(dir.path(), Some(3));

    gh_bin()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("3"));
}

// ── pull ──────────────────────────────────────────────────────────────────────

#[test]
fn test_gh_pull_without_sync_json_fails() {
    let dir = tempfile::tempdir().unwrap();

    gh_bin()
        .args(["pull"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("gh-sync.json"));
}

// ── push ──────────────────────────────────────────────────────────────────────

#[test]
fn test_gh_push_without_sync_json_fails() {
    let dir = tempfile::tempdir().unwrap();

    gh_bin()
        .args(["push"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("gh-sync.json"));
}

#[test]
fn test_gh_push_no_session_log() {
    let dir = tempfile::tempdir().unwrap();
    write_gh_sync(dir.path(), None);
    // No cryo.log — push reads log first (before any GH API call)

    gh_bin()
        .args(["push"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("No session log found"));
}

#[test]
fn test_gh_push_already_pushed_session() {
    let dir = tempfile::tempdir().unwrap();
    write_gh_sync(dir.path(), Some(2));
    write_minimal_log(dir.path());
    write_timer(dir.path(), 2);
    // session_num (2) == last_pushed_session (Some(2)) → should skip

    gh_bin()
        .args(["push"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("already pushed"));
}

// ── unsync without service ────────────────────────────────────────────────────

#[test]
fn test_gh_unsync_no_service() {
    let dir = tempfile::tempdir().unwrap();

    gh_bin()
        .args(["unsync"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("No sync service"));
}
