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
    };
    std::fs::write(messages_dir.join("a.md"), message_to_markdown(&valid)).unwrap();
    std::fs::write(messages_dir.join("b.md"), "not a valid message").unwrap();

    let messages = read_message_dir(&messages_dir, "message").unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "a.md");
    assert_eq!(messages[0].1.subject, "Question");
}
