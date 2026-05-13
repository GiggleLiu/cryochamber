use super::*;

#[test]
fn split_message_markdown_returns_frontmatter_and_trimmed_body() {
    let sections = split_message_markdown(
        "\n---\nfrom: human\nsubject: Test\n---\n\nBody line 1\nBody line 2\n",
    )
    .unwrap();

    assert_eq!(sections.frontmatter, "\nfrom: human\nsubject: Test");
    assert_eq!(sections.body, "Body line 1\nBody line 2");
}

#[test]
fn parse_frontmatter_fields_keeps_metadata_and_invalid_timestamp_fallback() {
    let fallback =
        NaiveDateTime::parse_from_str("2026-03-01T12:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();

    let fields = parse_frontmatter_fields(
        "\nfrom: human\nsubject: Test\ntimestamp: not-a-time\npriority: high\nignored\n",
        fallback,
    );

    assert_eq!(fields.from, "human");
    assert_eq!(fields.subject, "Test");
    assert_eq!(fields.timestamp, fallback);
    assert_eq!(fields.metadata.get("priority"), Some(&"high".to_string()));
}

#[test]
fn message_filename_base_uses_slug_when_subject_has_alphanumeric_text() {
    let msg = test_message("human", "Hello, World!", "Body", "2026-03-01T12:00:00");

    assert_eq!(
        message_filename_base(&msg),
        MessageFilenameBase::Slug("hello--world".to_string())
    );
}

#[test]
fn message_filename_base_uses_hash_when_subject_has_no_slug_content() {
    let msg = test_message("human", "!!!", "Body", "2026-03-01T12:00:00");

    assert_eq!(
        message_filename_base(&msg),
        MessageFilenameBase::Hash(message_hash(&msg))
    );
}

#[test]
fn slug_char_keeps_alphanumeric_characters() {
    assert_eq!(slug_char('a'), 'a');
    assert_eq!(slug_char('7'), '7');
}

#[test]
fn slug_char_replaces_separator_characters() {
    assert_eq!(slug_char(' '), '-');
    assert_eq!(slug_char('!'), '-');
}

#[test]
fn list_message_files_filters_markdown_files_and_sorts_by_filename() {
    let dir = tempfile::tempdir().unwrap();
    let messages_dir = dir.path().join("messages");
    std::fs::create_dir_all(messages_dir.join("nested.md")).unwrap();
    std::fs::write(messages_dir.join("b.md"), "b").unwrap();
    std::fs::write(messages_dir.join("a.md"), "a").unwrap();
    std::fs::write(messages_dir.join("ignore.txt"), "x").unwrap();

    let files = list_message_files(&messages_dir).unwrap();

    assert_eq!(
        files
            .iter()
            .map(|file| file.filename.as_str())
            .collect::<Vec<_>>(),
        vec!["a.md", "b.md"]
    );
    assert_eq!(files[0].path, messages_dir.join("a.md"));
}

#[test]
fn read_message_dir_skips_malformed_messages() {
    let dir = tempfile::tempdir().unwrap();
    let messages_dir = dir.path().join("messages");
    std::fs::create_dir_all(&messages_dir).unwrap();
    let valid = Message {
        from: "human".to_string(),
        subject: "Question".to_string(),
        body: "Body".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-03-01T12:00:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata: BTreeMap::new(),
        is_question: false,
    };
    std::fs::write(messages_dir.join("a.md"), message_to_markdown(&valid)).unwrap();
    std::fs::write(messages_dir.join("b.md"), "not a valid message").unwrap();

    let messages = read_message_dir(&messages_dir, "message").unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "a.md");
    assert_eq!(messages[0].1.subject, "Question");
}

#[test]
fn archive_outbox_messages_moves_files_to_outbox_archive() {
    let dir = tempfile::tempdir().unwrap();
    let msg = test_message("agent", "Reply", "Done", "2026-03-01T12:00:00");
    write_message(dir.path(), "outbox", &msg).unwrap();

    let outbox = read_outbox(dir.path()).unwrap();
    assert_eq!(outbox.len(), 1);
    let filename = outbox[0].0.clone();

    archive_outbox_messages(dir.path(), std::slice::from_ref(&filename)).unwrap();

    assert!(read_outbox(dir.path()).unwrap().is_empty());
    let archived = read_outbox_archive(dir.path()).unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].0, filename);
    assert_eq!(archived[0].1.body, "Done");
}

#[test]
fn archive_outbox_messages_ignores_missing_files_and_creates_archive_dir() {
    let dir = tempfile::tempdir().unwrap();

    archive_outbox_messages(dir.path(), &["missing.md".to_string()]).unwrap();

    assert!(dir.path().join("messages/outbox/archive").is_dir());
    assert!(read_outbox_archive(dir.path()).unwrap().is_empty());
}

#[test]
fn format_inbox_empty_returns_no_messages() {
    assert_eq!(format_inbox(&[]), "No messages.\n");
}

#[test]
fn format_inbox_single_message_includes_metadata_and_body() {
    let msg = test_message("alice", "hi", "Hello world", "2026-04-23T14:20:00");
    let out = format_inbox(&[("alice-2026-04-23T14-20-00.md".to_string(), msg)]);
    assert!(out.contains("--- alice-2026-04-23T14-20-00.md ---"));
    assert!(out.contains("From: alice"));
    assert!(out.contains("Subject: hi"));
    assert!(out.contains("Hello world"));
}

#[test]
fn format_inbox_multiple_messages_concatenates_in_order() {
    let a = test_message("a", "s1", "body1", "2026-04-23T14:20:00");
    let b = test_message("b", "s2", "body2", "2026-04-23T14:21:00");
    let out = format_inbox(&[("a.md".to_string(), a), ("b.md".to_string(), b)]);
    let pos_a = out.find("body1").unwrap();
    let pos_b = out.find("body2").unwrap();
    assert!(pos_a < pos_b, "messages should appear in input order");
}

#[test]
fn list_message_files_includes_subdir_entries_with_message_md() {
    let dir = tempfile::tempdir().unwrap();
    let messages_dir = dir.path().join("messages");
    std::fs::create_dir_all(&messages_dir).unwrap();

    std::fs::write(messages_dir.join("flat.md"), "x").unwrap();

    let sub = messages_dir.join("abc123");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("message.md"), "y").unwrap();

    let files = list_message_files(&messages_dir).unwrap();

    let names: Vec<&str> = files.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(names, vec!["abc123", "flat.md"]); // ASCII sort: '.' (0x2e) > nothing
    let abc = files.iter().find(|f| f.filename == "abc123").unwrap();
    assert_eq!(abc.path, sub.join("message.md"));
}

#[test]
fn list_message_files_skips_subdir_without_message_md() {
    let dir = tempfile::tempdir().unwrap();
    let messages_dir = dir.path().join("messages");
    std::fs::create_dir_all(&messages_dir).unwrap();
    std::fs::create_dir(messages_dir.join("incomplete")).unwrap();
    std::fs::write(messages_dir.join("incomplete").join("meta.json"), "{}").unwrap();

    assert!(list_message_files(&messages_dir).unwrap().is_empty());
}

#[test]
fn list_message_files_skips_archive_subdir() {
    let dir = tempfile::tempdir().unwrap();
    let messages_dir = dir.path().join("messages");
    std::fs::create_dir_all(messages_dir.join("archive")).unwrap();
    std::fs::write(messages_dir.join("archive").join("message.md"), "x").unwrap();

    assert!(list_message_files(&messages_dir).unwrap().is_empty());
}

#[test]
fn read_message_dir_sets_source_dir_metadata_for_subdir_entries() {
    let dir = tempfile::tempdir().unwrap();
    let messages_dir = dir.path().join("messages");
    std::fs::create_dir_all(&messages_dir).unwrap();

    let valid = test_message("alice", "Hi", "Body", "2026-04-23T14:20:00");
    std::fs::write(messages_dir.join("flat.md"), message_to_markdown(&valid)).unwrap();

    let sub = messages_dir.join("abc123");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("message.md"), message_to_markdown(&valid)).unwrap();

    let messages = read_message_dir(&messages_dir, "message").unwrap();
    assert_eq!(messages.len(), 2);

    let flat = messages.iter().find(|(name, _)| name == "flat.md").unwrap();
    assert!(!flat.1.metadata.contains_key("source_dir"));

    let subdir_msg = messages.iter().find(|(name, _)| name == "abc123").unwrap();
    let canonical = std::fs::canonicalize(&sub).unwrap();
    assert_eq!(
        subdir_msg.1.metadata.get("source_dir"),
        Some(&canonical.to_string_lossy().to_string())
    );
}

fn test_message(from: &str, subject: &str, body: &str, timestamp: &str) -> Message {
    Message {
        from: from.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
        timestamp: NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S").unwrap(),
        metadata: BTreeMap::new(),
        is_question: false,
    }
}

#[test]
fn parse_message_sets_is_question_when_frontmatter_question_true() {
    let raw = "---\nfrom: agent\nsubject: What is ice?\ntimestamp: 2026-04-25T15:30:00\n\
               question: true\n---\n\nWhat is ice?\n";

    let msg = parse_message(raw).unwrap();

    assert!(msg.is_question, "question: true should set is_question");
    assert!(
        !msg.metadata.contains_key("question"),
        "question flag should not leak into metadata"
    );
}

#[test]
fn parse_message_defaults_is_question_to_false_when_absent() {
    let raw = "---\nfrom: agent\nsubject: Status\ntimestamp: 2026-04-25T15:30:00\n---\n\nHi.\n";

    let msg = parse_message(raw).unwrap();

    assert!(!msg.is_question);
}

#[test]
fn parse_message_sets_is_question_false_when_question_false() {
    let raw = "---\nfrom: agent\nsubject: Status\ntimestamp: 2026-04-25T15:30:00\n\
               question: false\n---\n\nHi.\n";

    let msg = parse_message(raw).unwrap();

    assert!(!msg.is_question);
}

#[test]
fn message_to_markdown_emits_question_true_when_is_question_set() {
    let msg = Message {
        from: "agent".to_string(),
        subject: "What is ice?".to_string(),
        body: "What is ice?".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-04-25T15:30:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata: BTreeMap::new(),
        is_question: true,
    };

    let out = message_to_markdown(&msg);

    assert!(
        out.contains("question: true"),
        "expected question: true in frontmatter, got:\n{out}"
    );
}

#[test]
fn message_to_markdown_omits_question_field_when_is_question_false() {
    let msg = test_message("agent", "Status", "Hi.", "2026-04-25T15:30:00");

    let out = message_to_markdown(&msg);

    assert!(
        !out.contains("question:"),
        "expected no question field for non-question, got:\n{out}"
    );
}

#[test]
fn message_round_trip_preserves_is_question() {
    let msg = Message {
        from: "agent".to_string(),
        subject: "What is ice?".to_string(),
        body: "What is ice?".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-04-25T15:30:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata: BTreeMap::new(),
        is_question: true,
    };

    let parsed = parse_message(&message_to_markdown(&msg)).unwrap();

    assert!(parsed.is_question);
    assert_eq!(parsed.from, msg.from);
    assert_eq!(parsed.subject, msg.subject);
    assert_eq!(parsed.body, msg.body);
}

#[test]
fn parse_frontmatter_strips_surrounding_double_quotes() {
    let raw = "---\nfrom: \"Alice\"\nsubject: \"Hi there\"\ntimestamp: 2026-04-25T15:30:00\n---\n\nBody\n";
    let msg = parse_message(raw).unwrap();
    assert_eq!(msg.from, "Alice");
    assert_eq!(msg.subject, "Hi there");
}

#[test]
fn parse_frontmatter_keeps_unquoted_values_unchanged() {
    let raw = "---\nfrom: Alice\nsubject: Hi there\ntimestamp: 2026-04-25T15:30:00\n---\n\nBody\n";
    let msg = parse_message(raw).unwrap();
    assert_eq!(msg.from, "Alice");
    assert_eq!(msg.subject, "Hi there");
}

#[test]
fn parse_frontmatter_strips_only_outer_quote_pair() {
    let raw = "---\nfrom: \"Alice\\\"Q\\\" Smith\"\nsubject: Hi\ntimestamp: 2026-04-25T15:30:00\n---\n\nBody\n";
    let msg = parse_message(raw).unwrap();
    assert_eq!(msg.from, "Alice\"Q\" Smith");
}
