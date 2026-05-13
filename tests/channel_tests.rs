use chrono::NaiveDateTime;
use cryochamber::channel::store::MessageStore;
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
        is_question: false,
    }
}

#[test]
fn test_message_store_read_inbox_empty() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let channel = MessageStore::new(dir.path().to_path_buf());
    let messages = channel.read_inbox().unwrap();
    assert!(messages.is_empty());
}

#[test]
fn test_message_store_read_inbox_with_messages() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let msg = make_message(
        "human",
        "Question",
        "What about PR #42?",
        "2026-02-23T10:30:00",
    );
    message::write_message(dir.path(), "inbox", &msg).unwrap();

    let channel = MessageStore::new(dir.path().to_path_buf());
    let messages = channel.read_inbox().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "human");
}

#[test]
fn test_message_store_post_reply() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let channel = MessageStore::new(dir.path().to_path_buf());
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

#[test]
fn test_message_store_read_and_archive_inbox() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let msg = make_message("human", "Question", "Archive me", "2026-02-23T10:30:00");
    message::write_message(dir.path(), "inbox", &msg).unwrap();

    let store = MessageStore::new(dir.path().to_path_buf());
    let messages = store.read_and_archive_inbox().unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1.body, "Archive me");
    assert!(message::read_inbox(dir.path()).unwrap().is_empty());
    assert_eq!(message::read_inbox_archive(dir.path()).unwrap().len(), 1);
}

#[test]
fn test_message_store_send_out_writes_message() {
    let dir = tempfile::tempdir().unwrap();
    message::ensure_dirs(dir.path()).unwrap();
    let msg = make_message("agent", "Reply", "Send this out", "2026-02-23T10:45:00");

    let store = MessageStore::new(dir.path().to_path_buf());
    let path = store.send_out(&msg).unwrap();

    assert!(path.starts_with(dir.path().join("messages/outbox")));
    let outbox = message::read_outbox(dir.path()).unwrap();
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].1.body, "Send this out");
}

#[test]
fn test_message_store_covers_inbox_outbox_and_archives() {
    let dir = tempfile::tempdir().unwrap();
    let store = MessageStore::new(dir.path().to_path_buf());

    let inbox_msg = make_message("human", "Ping", "Inbox body", "2026-02-23T10:50:00");
    let inbox_path = store.send_in(&inbox_msg).unwrap();
    assert!(inbox_path.starts_with(dir.path().join("messages/inbox")));

    let out_msg = make_message("agent", "Reply", "Outbox body", "2026-02-23T10:55:00");
    store.send_out(&out_msg).unwrap();
    let outbox = store.read_outbox_named().unwrap();
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].1.body, "Outbox body");

    let inbox_named = store.read_inbox_named().unwrap();
    let inbox_filenames: Vec<String> = inbox_named.iter().map(|(name, _)| name.clone()).collect();
    store.archive_inbox(&inbox_filenames).unwrap();
    assert_eq!(store.read_inbox_archive_named().unwrap().len(), 1);

    let outbox_filenames: Vec<String> = outbox.iter().map(|(name, _)| name.clone()).collect();
    store.archive_outbox(&outbox_filenames).unwrap();
    assert_eq!(store.read_outbox_archive_named().unwrap().len(), 1);
}

#[test]
fn message_store_read_and_archive_inbox_handles_messenger_subdir() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let chamber = dir.path();
    let inbox = chamber.join("messages").join("inbox");
    fs::create_dir_all(&inbox).unwrap();

    let sub = inbox.join("mail-fixture1example-com");
    fs::create_dir(&sub).unwrap();
    fs::write(
        sub.join("message.md"),
        "---\nfrom: \"Alice\"\nsubject: \"Hello\"\ntimestamp: 2026-05-13T10:00:00\n---\n\nHi there.\n",
    )
    .unwrap();
    fs::write(
        sub.join("meta.json"),
        r#"{"matched_by":"llm","confidence":0.9,"reason":"x","tags":[]}"#,
    )
    .unwrap();

    let store = MessageStore::new(chamber.to_path_buf());
    let messages = store.read_and_archive_inbox().unwrap();

    assert_eq!(messages.len(), 1);
    let (filename, msg) = &messages[0];
    assert_eq!(filename, "mail-fixture1example-com");
    assert_eq!(msg.from, "Alice");
    assert_eq!(msg.subject, "Hello");
    assert_eq!(msg.body.trim(), "Hi there.");
    assert!(msg.metadata.contains_key("source_dir"));

    assert!(!sub.exists());
    let archived = inbox.join("archive").join("mail-fixture1example-com");
    assert!(archived.join("message.md").is_file());
    assert!(archived.join("meta.json").is_file());
}
