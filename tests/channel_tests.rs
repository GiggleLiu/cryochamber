use chrono::NaiveDateTime;
use cryochamber::channel::file::FileChannel;
use cryochamber::channel::MessageChannel;
use cryochamber::message::{self, Message};
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
fn test_file_channel_read_inbox_empty() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let channel = FileChannel::new(dir.path().to_path_buf());
    let messages = channel.read_inbox().unwrap();
    assert!(messages.is_empty());
}

#[test]
fn test_file_channel_read_inbox_with_messages() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let msg = make_message(
        "human",
        "Question",
        "What about PR #42?",
        "2026-02-23T10:30:00",
    );
    message::write_message(dir.path(), "inbox", &msg).unwrap();

    let channel = FileChannel::new(dir.path().to_path_buf());
    let messages = channel.read_inbox().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "human");
}

#[test]
fn test_file_channel_post_reply() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let channel = FileChannel::new(dir.path().to_path_buf());
    channel.post_reply("Session 3 complete.").unwrap();

    // Verify it ended up in outbox
    let outbox = dir.path().join("messages/outbox");
    let entries: Vec<_> = std::fs::read_dir(&outbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert_eq!(entries.len(), 1);

    let content = std::fs::read_to_string(entries[0].path()).unwrap();
    assert!(content.contains("Session 3 complete."));
}
