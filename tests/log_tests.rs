// tests/log_tests.rs
use cryochamber::log::{read_latest_session, session_count, EventLogger};
use std::fs;

#[test]
fn test_event_logger_creates_session() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let mut logger = EventLogger::begin(&log_path, 1, "Review PRs", "opencode run", &[]).unwrap();
    logger.log_event("agent started (pid 1234)").unwrap();
    logger
        .log_event("hibernate: wake=2026-03-08T09:00, exit=0")
        .unwrap();
    logger.finish("agent exited (code 0)").unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("--- CRYO SESSION 1"));
    assert!(contents.contains("task: Review PRs"));
    assert!(contents.contains("agent: opencode run"));
    assert!(contents.contains("--- CRYO END ---"));
}

#[test]
fn test_multiple_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let logger1 = EventLogger::begin(&log_path, 1, "Task one", "agent", &[]).unwrap();
    logger1.finish("done").unwrap();

    let logger2 = EventLogger::begin(&log_path, 2, "Task two", "agent", &[]).unwrap();
    logger2.finish("done").unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert_eq!(contents.matches("--- CRYO SESSION").count(), 2);
    assert_eq!(contents.matches("--- CRYO END ---").count(), 2);
}

#[test]
fn test_read_latest_session() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let mut logger1 = EventLogger::begin(&log_path, 1, "Task one", "agent", &[]).unwrap();
    logger1.log_event("first session work").unwrap();
    logger1.finish("done").unwrap();

    let mut logger2 = EventLogger::begin(&log_path, 2, "Task two", "agent", &[]).unwrap();
    logger2.log_event("second session work").unwrap();
    logger2.finish("done").unwrap();

    let latest = read_latest_session(&log_path).unwrap().unwrap();
    assert!(latest.contains("second session work"));
    assert!(!latest.contains("first session work"));
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

    assert_eq!(session_count(&log_path).unwrap(), 0);

    let logger1 = EventLogger::begin(&log_path, 1, "T", "agent", &[]).unwrap();
    logger1.finish("done").unwrap();
    assert_eq!(session_count(&log_path).unwrap(), 1);

    let logger2 = EventLogger::begin(&log_path, 2, "T", "agent", &[]).unwrap();
    logger2.finish("done").unwrap();
    assert_eq!(session_count(&log_path).unwrap(), 2);
}

#[test]
fn test_event_logger_with_inbox_filenames() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let logger = EventLogger::begin(
        &log_path,
        1,
        "Review PRs",
        "claude -p",
        &["msg1.md".to_string(), "msg2.md".to_string()],
    )
    .unwrap();
    logger.finish("done").unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("inbox: 2 messages (msg1.md, msg2.md)"));
}

#[test]
fn test_event_logger_no_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let logger = EventLogger::begin(&log_path, 1, "task", "agent", &[]).unwrap();
    logger.finish("done").unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("inbox: 0 messages"));
}
