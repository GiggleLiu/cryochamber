// tests/message_tests.rs
use chrono::NaiveDateTime;
use cryochamber::message::{
    archive_messages, ensure_dirs, message_to_markdown, parse_message, read_inbox, write_message,
    Message,
};
use std::collections::BTreeMap;

fn make_message(from: &str, subject: &str, body: &str, ts: &str) -> Message {
    Message {
        from: from.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
        timestamp: NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S").unwrap(),
        metadata: BTreeMap::new(),
    }
}

#[test]
fn test_ensure_dirs() {
    let dir = tempfile::tempdir().unwrap();
    ensure_dirs(dir.path()).unwrap();
    assert!(dir.path().join("messages/inbox").is_dir());
    assert!(dir.path().join("messages/outbox").is_dir());
    assert!(dir.path().join("messages/inbox/archive").is_dir());
}

#[test]
fn test_ensure_dirs_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    ensure_dirs(dir.path()).unwrap();
    ensure_dirs(dir.path()).unwrap(); // should not fail
}

#[test]
fn test_write_and_read_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let msg = make_message(
        "human",
        "Question",
        "What about PR #42?",
        "2026-02-23T10:30:00",
    );
    write_message(dir.path(), "inbox", &msg).unwrap();

    let inbox = read_inbox(dir.path()).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].1.from, "human");
    assert_eq!(inbox[0].1.subject, "Question");
    assert!(inbox[0].1.body.contains("PR #42"));
}

#[test]
fn test_read_inbox_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let msg_late = make_message("bot", "Late", "Later msg", "2026-02-23T12:00:00");
    let msg_early = make_message("human", "Early", "Earlier msg", "2026-02-23T08:00:00");
    // Write late first, then early
    write_message(dir.path(), "inbox", &msg_late).unwrap();
    write_message(dir.path(), "inbox", &msg_early).unwrap();

    let inbox = read_inbox(dir.path()).unwrap();
    assert_eq!(inbox.len(), 2);
    // Sorted by filename (timestamp), so early comes first
    assert_eq!(inbox[0].1.from, "human");
    assert_eq!(inbox[1].1.from, "bot");
}

#[test]
fn test_read_inbox_empty() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = read_inbox(dir.path()).unwrap();
    assert!(inbox.is_empty());
}

#[test]
fn test_read_inbox_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    ensure_dirs(dir.path()).unwrap();
    let inbox = read_inbox(dir.path()).unwrap();
    assert!(inbox.is_empty());
}

#[test]
fn test_archive_messages() {
    let dir = tempfile::tempdir().unwrap();
    let msg = make_message("human", "Test", "Hello", "2026-02-23T10:00:00");
    write_message(dir.path(), "inbox", &msg).unwrap();

    let inbox = read_inbox(dir.path()).unwrap();
    assert_eq!(inbox.len(), 1);
    let filename = inbox[0].0.clone();

    archive_messages(dir.path(), std::slice::from_ref(&filename)).unwrap();

    // Inbox should be empty now
    let inbox_after = read_inbox(dir.path()).unwrap();
    assert!(inbox_after.is_empty());

    // Archive should have the file
    let archive_path = dir.path().join("messages/inbox/archive").join(&filename);
    assert!(archive_path.exists());
}

#[test]
fn test_message_roundtrip() {
    let mut metadata = BTreeMap::new();
    metadata.insert("priority".to_string(), "high".to_string());

    let msg = Message {
        from: "bot".to_string(),
        subject: "Alert".to_string(),
        body: "Something happened.".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-02-23T15:30:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata,
    };

    let markdown = message_to_markdown(&msg);
    let parsed = parse_message(&markdown).unwrap();

    assert_eq!(parsed.from, "bot");
    assert_eq!(parsed.subject, "Alert");
    assert_eq!(parsed.body, "Something happened.");
    assert_eq!(parsed.metadata.get("priority"), Some(&"high".to_string()));
}

#[test]
fn test_write_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let mut metadata = BTreeMap::new();
    metadata.insert("fallback_action".to_string(), "email".to_string());
    metadata.insert(
        "fallback_target".to_string(),
        "user@example.com".to_string(),
    );

    let msg = Message {
        from: "cryochamber".to_string(),
        subject: "Fallback Alert".to_string(),
        body: "Session did not run".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-02-23T12:00:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata,
    };

    let path = write_message(dir.path(), "outbox", &msg).unwrap();
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("fallback_action: email"));
    assert!(content.contains("fallback_target: user@example.com"));
    assert!(content.contains("Session did not run"));
}

#[test]
fn test_parse_message_missing_frontmatter() {
    let result = parse_message("Just some text without frontmatter");
    assert!(result.is_err());
}

#[test]
fn test_empty_subject_uses_hash_disambiguator() {
    let dir = tempfile::tempdir().unwrap();
    // Two messages with empty subject in the same second — should NOT collide
    let msg1 = make_message("alice", "", "First message", "2026-02-23T10:00:00");
    let msg2 = make_message("bob", "", "Second message", "2026-02-23T10:00:00");

    let path1 = write_message(dir.path(), "inbox", &msg1).unwrap();
    let path2 = write_message(dir.path(), "inbox", &msg2).unwrap();

    assert_ne!(
        path1, path2,
        "Different messages should produce different filenames"
    );

    let inbox = read_inbox(dir.path()).unwrap();
    assert_eq!(inbox.len(), 2);
}

#[test]
fn test_empty_subject_same_content_same_hash() {
    let dir = tempfile::tempdir().unwrap();
    // Same body and author at same timestamp — should produce same filename (overwrite is expected)
    let msg1 = make_message("alice", "", "Same content", "2026-02-23T10:00:00");
    let msg2 = make_message("alice", "", "Same content", "2026-02-23T10:00:00");

    let path1 = write_message(dir.path(), "inbox", &msg1).unwrap();
    let path2 = write_message(dir.path(), "inbox", &msg2).unwrap();

    assert_eq!(
        path1, path2,
        "Identical messages should produce same filename"
    );
}

#[test]
fn test_filename_no_colons() {
    let dir = tempfile::tempdir().unwrap();
    let msg = make_message("human", "Test", "Body", "2026-02-23T10:30:00");
    let path = write_message(dir.path(), "inbox", &msg).unwrap();
    let filename = path.file_name().unwrap().to_string_lossy();
    assert!(
        !filename.contains(':'),
        "Filename should not contain colons: {filename}"
    );
    assert!(filename.starts_with("2026-02-23T10-30-00"));
}
