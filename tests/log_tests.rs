// tests/log_tests.rs
use cryochamber::log::{
    parse_latest_session_notes, parse_latest_session_task, parse_latest_session_wake,
    read_current_session, read_latest_session, session_count, EventLogger,
};
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

#[test]
fn test_parse_latest_session_notes() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let mut logger = EventLogger::begin(&log_path, 1, "Build feature", "agent", &[]).unwrap();
    logger.log_event("agent started (pid 1234)").unwrap();
    logger.log_event("note: \"First note\"").unwrap();
    logger.log_event("note: \"Second note\"").unwrap();
    logger.finish("done").unwrap();

    let notes = parse_latest_session_notes(&log_path).unwrap();
    assert_eq!(notes, vec!["First note", "Second note"]);
}

#[test]
fn test_parse_latest_session_notes_empty() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let logger = EventLogger::begin(&log_path, 1, "task", "agent", &[]).unwrap();
    logger.finish("done").unwrap();

    let notes = parse_latest_session_notes(&log_path).unwrap();
    assert!(notes.is_empty());
}

#[test]
fn test_parse_latest_session_notes_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("nonexistent.log");

    let notes = parse_latest_session_notes(&log_path).unwrap();
    assert!(notes.is_empty());
}

#[test]
fn test_parse_latest_session_notes_only_latest() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let mut logger1 = EventLogger::begin(&log_path, 1, "task1", "agent", &[]).unwrap();
    logger1.log_event("note: \"Old note\"").unwrap();
    logger1.finish("done").unwrap();

    let mut logger2 = EventLogger::begin(&log_path, 2, "task2", "agent", &[]).unwrap();
    logger2.log_event("note: \"New note\"").unwrap();
    logger2.finish("done").unwrap();

    let notes = parse_latest_session_notes(&log_path).unwrap();
    assert_eq!(notes, vec!["New note"]);
}

#[test]
fn test_parse_latest_session_task() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let logger = EventLogger::begin(&log_path, 1, "Review PRs", "agent", &[]).unwrap();
    logger.finish("done").unwrap();

    let task = parse_latest_session_task(&log_path).unwrap();
    assert_eq!(task, Some("Review PRs".to_string()));
}

#[test]
fn test_parse_latest_session_task_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("nonexistent.log");

    let task = parse_latest_session_task(&log_path).unwrap();
    assert!(task.is_none());
}

#[test]
fn test_read_current_session_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    // Complete first session
    let logger1 = EventLogger::begin(&log_path, 1, "Old task", "agent", &[]).unwrap();
    logger1.finish("done").unwrap();

    // Start second session without finishing (in-progress)
    let mut logger2 = EventLogger::begin(&log_path, 2, "Current task", "agent", &[]).unwrap();
    logger2.log_event("note: \"WIP note\"").unwrap();

    // read_latest_session returns None (no completed latest session)
    assert!(read_latest_session(&log_path).unwrap().is_none());

    // read_current_session returns the in-progress session
    let current = read_current_session(&log_path).unwrap().unwrap();
    assert!(current.contains("Current task"));
    assert!(current.contains("WIP note"));

    // task and notes should work from in-progress session
    let task = parse_latest_session_task(&log_path).unwrap();
    assert_eq!(task, Some("Current task".to_string()));

    let notes = parse_latest_session_notes(&log_path).unwrap();
    assert_eq!(notes, vec!["WIP note"]);

    // Suppress drop warning by finishing
    logger2.finish("done").unwrap();
}

#[test]
fn test_notes_fallback_to_previous_session() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    // Session 1: has notes
    let mut logger1 = EventLogger::begin(&log_path, 1, "task1", "agent", &[]).unwrap();
    logger1.log_event("note: \"Important note\"").unwrap();
    logger1
        .log_event("hibernate: wake=2026-03-01T09:00, exit=0, summary=\"done\"")
        .unwrap();
    logger1.finish("session complete").unwrap();

    // Session 2: just started (no notes, no hibernate)
    let _logger2 = EventLogger::begin(&log_path, 2, "task2", "agent", &[]).unwrap();

    // Notes should fall back to session 1
    let notes = parse_latest_session_notes(&log_path).unwrap();
    assert_eq!(notes, vec!["Important note"]);

    // Wake should find session 1's hibernate line
    let wake = parse_latest_session_wake(&log_path).unwrap();
    assert_eq!(wake, Some("2026-03-01T09:00".to_string()));

    _logger2.finish("done").unwrap();
}

#[test]
fn test_parse_latest_session_wake() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let mut logger = EventLogger::begin(&log_path, 1, "task", "agent", &[]).unwrap();
    logger
        .log_event("hibernate: wake=2026-03-01T09:00, exit=0, summary=\"done\"")
        .unwrap();
    logger.finish("session complete").unwrap();

    let wake = parse_latest_session_wake(&log_path).unwrap();
    assert_eq!(wake, Some("2026-03-01T09:00".to_string()));
}

#[test]
fn test_parse_latest_session_wake_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    // Session in progress — hibernate logged but no CRYO END yet
    let mut logger = EventLogger::begin(&log_path, 1, "task", "agent", &[]).unwrap();
    logger
        .log_event("hibernate: wake=2026-03-01T10:00, exit=0, summary=\"waiting\"")
        .unwrap();
    // Don't call finish — simulates daemon still processing

    let wake = parse_latest_session_wake(&log_path).unwrap();
    assert_eq!(wake, Some("2026-03-01T10:00".to_string()));

    logger.finish("done").unwrap();
}

#[test]
fn test_parse_latest_session_wake_none() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let logger = EventLogger::begin(&log_path, 1, "task", "agent", &[]).unwrap();
    logger.finish("done").unwrap();

    let wake = parse_latest_session_wake(&log_path).unwrap();
    assert!(wake.is_none());
}

#[test]
fn test_parse_latest_session_wake_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("nonexistent.log");

    let wake = parse_latest_session_wake(&log_path).unwrap();
    assert!(wake.is_none());
}
