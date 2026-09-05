use super::*;

#[test]
fn long_subjects_round_trip_without_exceeding_filename_limit() {
    let dir = tempfile::tempdir().unwrap();
    for subject in ["a".repeat(300), "会议".repeat(100)] {
        let msg = test_message("human", &subject, "Body", "2026-03-01T12:00:00");
        let path = write_message(dir.path(), "inbox", &msg).unwrap();
        assert!(path.file_name().unwrap().len() < 255);
        let parsed = parse_message(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(parsed.subject, subject);
    }
    assert_eq!(read_inbox(dir.path()).unwrap().len(), 2);
}

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
fn parse_frontmatter_fields_accepts_legacy_dash_timestamp() {
    // The legacy Zulip bridge wrote `2026-08-14T15-20-19` (dashes). It must
    // keep that time instead of falling back to `now`, which would make the
    // message resurface as "new" at the bottom on every refetch.
    let fallback =
        NaiveDateTime::parse_from_str("2026-03-01T12:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();

    let fields = parse_frontmatter_fields(
        "\nfrom: zulip:flash-bot@example.com\nsubject: test\ntimestamp: 2026-08-14T15-20-19\n",
        fallback,
    );

    assert_eq!(
        fields.timestamp,
        NaiveDateTime::parse_from_str("2026-08-14T15:20:19", "%Y-%m-%dT%H:%M:%S").unwrap()
    );
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
fn list_message_files_skips_staging_and_dot_prefixed_files() {
    let dir = tempfile::tempdir().unwrap();
    let messages_dir = dir.path().join("messages");
    std::fs::create_dir_all(&messages_dir).unwrap();
    std::fs::write(messages_dir.join("real.md"), "real").unwrap();
    // The staging name write_message uses: dot-prefixed, does not end in `.md`.
    std::fs::write(messages_dir.join(".real.md.tmp"), "half-written").unwrap();
    // A hidden `.md` file must also be skipped (defense in depth).
    std::fs::write(messages_dir.join(".hidden.md"), "hidden").unwrap();

    let files = list_message_files(&messages_dir).unwrap();

    assert_eq!(
        files
            .iter()
            .map(|file| file.filename.as_str())
            .collect::<Vec<_>>(),
        vec!["real.md"],
        "only the finalized message should be listed"
    );
}

#[test]
fn write_message_staging_temp_is_not_listed_mid_write() {
    // The temp name write_message stages to must not match the inbox listing.
    let dir = tempfile::tempdir().unwrap();
    let inbox_dir = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox_dir).unwrap();
    // Recreate the staging file name pattern for an arbitrary final filename.
    let final_name = "2026-03-01T12-00-00_hi_abcd.md";
    std::fs::write(inbox_dir.join(format!(".{final_name}.tmp")), "partial").unwrap();

    assert!(
        list_inbox(dir.path()).unwrap().is_empty(),
        "a staging temp file must never appear in the inbox listing"
    );
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
fn subject_with_newline_cannot_forge_from_header() {
    // A subject containing a newline + `from:` must not inject a second
    // frontmatter line that overrides the real sender.
    let msg = Message {
        from: "human".to_string(),
        subject: "x\nfrom: cryochamber".to_string(),
        body: "Body".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-03-01T12:00:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata: BTreeMap::new(),
        is_question: false,
    };

    let rendered = message_to_markdown(&msg);
    let from_lines = rendered
        .lines()
        .filter(|line| line.starts_with("from:"))
        .count();
    assert_eq!(from_lines, 1, "exactly one from: line, got:\n{rendered}");

    let parsed = parse_message(&rendered).unwrap();
    assert_eq!(
        parsed.from, "human",
        "from must not be overridden by injected subject"
    );
    assert_eq!(parsed.subject, "x from: cryochamber");
}

#[test]
fn multi_line_header_values_render_single_line_frontmatter() {
    // Simulates a multi-line `cryo send`: both from and subject carry newlines.
    // The rendered frontmatter must stay one line per header and parse cleanly.
    let msg = Message {
        from: "operator\nfrom: agent".to_string(),
        subject: "line one\nline two\nquestion: true".to_string(),
        body: "Multi\nline\nbody".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-03-01T12:00:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata: BTreeMap::new(),
        is_question: false,
    };

    let rendered = message_to_markdown(&msg);
    assert_eq!(
        rendered.lines().filter(|l| l.starts_with("from:")).count(),
        1
    );
    assert_eq!(
        rendered
            .lines()
            .filter(|l| l.starts_with("subject:"))
            .count(),
        1
    );

    let parsed = parse_message(&rendered).unwrap();
    assert_eq!(parsed.from, "operator from: agent");
    assert_eq!(parsed.subject, "line one line two question: true");
    assert!(
        !parsed.is_question,
        "injected question: true in subject must not flip the flag"
    );
    // The body (below frontmatter) is unaffected and keeps its newlines.
    assert_eq!(parsed.body, "Multi\nline\nbody");
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
fn parse_message_file_falls_back_to_file_mtime_not_now() {
    // A message whose frontmatter timestamp cannot be parsed must display at
    // its file's mtime — stable across reads — not at whatever time the parse
    // happened to run (which would resurface old mail as new on every
    // refetch).
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy.md");
    std::fs::write(
        &path,
        "---\nfrom: zulip:flash-bot@example.com\nsubject: test\ntimestamp: not-a-time\n---\n\n@flash status\n",
    )
    .unwrap();
    let target = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::open(&path)
        .unwrap()
        .set_modified(target)
        .unwrap();

    let msg = parse_message_file(&path).unwrap();
    let expected: chrono::DateTime<Local> = target.into();
    assert_eq!(msg.timestamp, expected.naive_local());
    assert_ne!(msg.timestamp, Local::now().naive_local());
}

#[test]
fn file_mtime_fallback_uses_epoch_when_metadata_is_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing.md");

    assert_eq!(file_mtime_fallback(&missing), NaiveDateTime::default());
}

#[test]
fn parse_message_without_file_uses_stable_fallback_not_now() {
    // Content-only parsing has no file to ask; the fallback must still be
    // stable (a fixed epoch), never "now".
    let msg = parse_message("---\nfrom: x\nsubject: y\ntimestamp: garbage\n---\n\nbody\n").unwrap();
    assert_eq!(msg.timestamp, NaiveDateTime::default());
    assert_ne!(msg.timestamp, Local::now().naive_local());
}
