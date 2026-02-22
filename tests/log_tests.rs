// tests/log_tests.rs
use cryochamber::log::{append_session, read_latest_session, Session};
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn test_append_session_to_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let session = Session {
        number: 1,
        task: "Review PRs".to_string(),
        output: "Reviewed 3 PRs.\n[CRYO:EXIT 0] Done".to_string(),
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
    };
    let s2 = Session {
        number: 2,
        task: "Task two".to_string(),
        output: "[CRYO:EXIT 0] Also done".to_string(),
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
    };
    let s2 = Session {
        number: 2,
        task: "Task two".to_string(),
        output: "[CRYO:EXIT 0] Second".to_string(),
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

    let s1 = Session { number: 1, task: "T".into(), output: "O".into() };
    append_session(&log_path, &s1).unwrap();
    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 1);

    let s2 = Session { number: 2, task: "T".into(), output: "O".into() };
    append_session(&log_path, &s2).unwrap();
    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 2);
}
