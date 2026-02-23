// tests/log_tests.rs
use cryochamber::log::{append_session, read_latest_session, Session};
use std::fs;

#[test]
fn test_append_session_to_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let session = Session {
        number: 1,
        task: "Review PRs".to_string(),
        output: "Reviewed 3 PRs.\n[CRYO:EXIT 0] Done".to_string(),
        stderr: None,
        inbox_filenames: vec![],
    };

    append_session(&log_path, &session).unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("--- CRYO SESSION"));
    assert!(contents.contains("Session: 1"));
    assert!(contents.contains("Task: Review PRs"));
    assert!(contents.contains("[CRYO:EXIT 0] Done"));
    assert!(contents.contains("--- CRYO END ---"));
}

#[test]
fn test_append_multiple_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let s1 = Session {
        number: 1,
        task: "Task one".to_string(),
        output: "[CRYO:EXIT 0] Done".to_string(),
        stderr: None,
        inbox_filenames: vec![],
    };
    let s2 = Session {
        number: 2,
        task: "Task two".to_string(),
        output: "[CRYO:EXIT 0] Also done".to_string(),
        stderr: None,
        inbox_filenames: vec![],
    };

    append_session(&log_path, &s1).unwrap();
    append_session(&log_path, &s2).unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert_eq!(contents.matches("--- CRYO SESSION").count(), 2);
    assert_eq!(contents.matches("--- CRYO END ---").count(), 2);
}

#[test]
fn test_read_latest_session() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let s1 = Session {
        number: 1,
        task: "Task one".to_string(),
        output: "[CRYO:EXIT 0] First".to_string(),
        stderr: None,
        inbox_filenames: vec![],
    };
    let s2 = Session {
        number: 2,
        task: "Task two".to_string(),
        output: "[CRYO:EXIT 0] Second".to_string(),
        stderr: None,
        inbox_filenames: vec![],
    };

    append_session(&log_path, &s1).unwrap();
    append_session(&log_path, &s2).unwrap();

    let latest = read_latest_session(&log_path).unwrap().unwrap();
    assert!(latest.contains("Second"));
    assert!(!latest.contains("First"));
}

#[test]
fn test_read_latest_session_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");
    fs::write(&log_path, "").unwrap();

    let latest = read_latest_session(&log_path).unwrap();
    assert!(latest.is_none());
}

#[test]
fn test_read_latest_session_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("nonexistent.log");

    let latest = read_latest_session(&log_path).unwrap();
    assert!(latest.is_none());
}

#[test]
fn test_session_count() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 0);

    let s1 = Session {
        number: 1,
        task: "T".into(),
        output: "O".into(),
        stderr: None,
        inbox_filenames: vec![],
    };
    append_session(&log_path, &s1).unwrap();
    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 1);

    let s2 = Session {
        number: 2,
        task: "T".into(),
        output: "O".into(),
        stderr: None,
        inbox_filenames: vec![],
    };
    append_session(&log_path, &s2).unwrap();
    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 2);
}

#[test]
fn test_append_and_read_latest_with_markers() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let session = Session {
        number: 1,
        task: "Review PRs".to_string(),
        output: "Did work.\n[CRYO:EXIT 0] All done\n[CRYO:WAKE 2026-03-08T09:00]\n[CRYO:CMD opencode test]\n[CRYO:PLAN check status]".to_string(),
        stderr: None,
        inbox_filenames: vec![],
    };

    append_session(&log_path, &session).unwrap();
    let latest = read_latest_session(&log_path).unwrap().unwrap();
    assert!(latest.contains("[CRYO:EXIT 0]"));
    assert!(latest.contains("[CRYO:WAKE 2026-03-08T09:00]"));
    assert!(latest.contains("[CRYO:CMD opencode test]"));
    assert!(latest.contains("[CRYO:PLAN check status]"));
}

#[test]
fn test_session_with_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let session = Session {
        number: 1,
        task: "Run agent".to_string(),
        output: "[CRYO:EXIT 1] Partial".to_string(),
        stderr: Some("Warning: rate limited\nError: timeout".to_string()),
        inbox_filenames: vec![],
    };
    append_session(&log_path, &session).unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("--- STDERR ---"));
    assert!(contents.contains("Warning: rate limited"));
    assert!(contents.contains("Error: timeout"));
}

#[test]
fn test_session_empty_stderr_not_logged() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let session = Session {
        number: 1,
        task: "Run agent".to_string(),
        output: "[CRYO:EXIT 0] Done".to_string(),
        stderr: Some("  \n  ".to_string()),
        inbox_filenames: vec![],
    };
    append_session(&log_path, &session).unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert!(!contents.contains("--- STDERR ---"));
}
