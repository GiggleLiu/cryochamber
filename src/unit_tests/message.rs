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
