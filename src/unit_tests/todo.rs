use super::*;

#[test]
fn test_backward_compat_missing_at_field() {
    // Legacy JSON without the `at` field should deserialize with default empty string.
    let json = r#"[{"id":1,"text":"old item","done":false,"created":"unknown"}]"#;
    let items: Vec<TodoItem> = serde_json::from_str(json).unwrap();
    assert_eq!(items[0].at, "", "Missing at should default to empty string");
    assert!(!items[0].claimed, "Missing claimed should default to false");
}

#[test]
fn test_todo_file_add_dedups_open_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = TodoFile::new(&path);

    let first = todos
        .add("[internal] heartbeat".into(), "2026-03-02T21:01".into())
        .unwrap();
    let second = todos
        .add("[internal] heartbeat".into(), "2026-03-02T21:01".into())
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(todos.items().unwrap().len(), 1);

    todos.done(first).unwrap();
    let third = todos
        .add("[internal] heartbeat".into(), "2026-03-02T21:01".into())
        .unwrap();
    assert_ne!(first, third);
    assert_eq!(todos.items().unwrap().len(), 2);
}

#[test]
fn test_todo_file_next_valid_wake_skips_invalid_and_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    std::fs::write(
        &path,
        r#"[{"id":1,"text":"legacy","done":false,"created":"unknown"},{"id":2,"text":"bad","done":false,"at":"2026-03-02 10:00","created":"unknown"},{"id":3,"text":"good","done":false,"at":"2026-03-02T14:00","created":"unknown"}]"#,
    )
    .unwrap();

    let wake = TodoFile::new(&path).next_valid_wake().unwrap();
    assert_eq!(
        wake,
        Some(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(14, 0, 0)
                .unwrap()
        )
    );
}

#[test]
fn test_todo_file_next_valid_wake_accepts_seconds_precision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    std::fs::write(
        &path,
        r#"[{"id":1,"text":"with seconds","done":false,"at":"2026-03-02T09:07:00","created":"unknown"}]"#,
    )
    .unwrap();

    let wake = TodoFile::new(&path).next_valid_wake().unwrap();
    assert_eq!(
        wake,
        Some(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(9, 7, 0)
                .unwrap()
        )
    );
}

#[test]
fn test_todo_file_round_trips_direct_file_operations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = TodoFile::new(&path);

    let due_id = todos
        .add("due task".to_string(), "2026-03-02T10:00".to_string())
        .unwrap();
    let future_id = todos
        .add("future task".to_string(), "2026-03-02T12:00".to_string())
        .unwrap();

    assert_eq!(
        todos.next_wake_time().unwrap().as_deref(),
        Some("2026-03-02T10:00")
    );

    let claimed = todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();
    assert_eq!(
        claimed
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        vec!["due task"]
    );

    let display = todos.display().unwrap();
    assert!(display.contains("1. [~] due task (at: 2026-03-02T10:00)"));
    assert!(display.contains("2. [ ] future task (at: 2026-03-02T12:00)"));
    assert_eq!(
        todos.next_wake_time().unwrap().as_deref(),
        Some("2026-03-02T12:00")
    );

    let completed = todos.complete_claimed().unwrap();
    assert_eq!(completed.len(), 1);

    let display = todos.display().unwrap();
    assert!(display.contains("1. [x] due task (at: 2026-03-02T10:00)"));

    todos.done(future_id).unwrap();

    let display = todos.display().unwrap();
    assert!(display.contains("1. [x] due task (at: 2026-03-02T10:00)"));
    assert!(display.contains("2. [x] future task (at: 2026-03-02T12:00)"));

    assert_eq!(due_id, 1);
}

#[test]
fn test_todo_file_claim_due_claims_due_items_and_returns_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = TodoFile::new(&path);

    todos
        .add("first due".to_string(), "2026-03-02T10:00".to_string())
        .unwrap();
    todos
        .add("future".to_string(), "2026-03-02T12:00".to_string())
        .unwrap();
    todos
        .add("second due".to_string(), "2026-03-02T10:30".to_string())
        .unwrap();

    let popped = todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:45", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();

    assert_eq!(popped.len(), 2);
    assert_eq!(popped[0].text, "first due");
    assert!(popped[0].claimed);
    assert!(!popped[0].done);
    assert_eq!(popped[1].text, "second due");
    assert!(popped[1].claimed);
    assert!(!popped[1].done);

    let items = todos.items().unwrap();
    assert!(!items[0].done);
    assert!(items[0].claimed);
    assert!(!items[1].done);
    assert!(!items[1].claimed);
    assert!(!items[2].done);
    assert!(items[2].claimed);

    assert!(todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:45", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap()
        .is_empty());
}

#[test]
fn test_todo_file_claim_due_accepts_seconds_precision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = TodoFile::new(&path);

    todos
        .add(
            "due with seconds".to_string(),
            "2026-03-02T10:00:00".to_string(),
        )
        .unwrap();

    let popped = todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();

    assert_eq!(popped.len(), 1);
    assert_eq!(popped[0].text, "due with seconds");
}

#[test]
fn test_todo_file_next_valid_wake_ignores_claimed_items() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = TodoFile::new(&path);

    todos
        .add("due".to_string(), "2026-03-02T10:00".to_string())
        .unwrap();
    todos
        .add("future".to_string(), "2026-03-02T12:00".to_string())
        .unwrap();

    todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();

    assert_eq!(
        todos.next_valid_wake().unwrap(),
        Some(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
        )
    );
}

#[test]
fn test_todo_file_complete_claimed_marks_claimed_items_done() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = TodoFile::new(&path);

    todos
        .add("due".to_string(), "2026-03-02T10:00".to_string())
        .unwrap();
    todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();

    let completed = todos.complete_claimed().unwrap();
    assert_eq!(completed.len(), 1);

    let items = todos.items().unwrap();
    assert!(items[0].done);
    assert!(!items[0].claimed);
    assert_eq!(
        todos.display().unwrap(),
        "1. [x] due (at: 2026-03-02T10:00)"
    );
}

#[test]
fn test_todo_file_reschedule_claimed_after_crash_marks_claimed_done_and_adds_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = TodoFile::new(&path);

    todos
        .add("due".to_string(), "2026-03-02T10:00".to_string())
        .unwrap();
    todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();

    let retry_ids = todos
        .reschedule_claimed_after_crash(
            chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();

    assert_eq!(retry_ids, vec![2]);
    let items = todos.items().unwrap();
    assert!(items[0].done);
    assert!(!items[0].claimed);
    assert_eq!(items[1].text, "due (attempt 1)");
    assert_eq!(items[1].at, "2026-03-02T10:32");
    assert!(!items[1].done);
    assert!(!items[1].claimed);
}
