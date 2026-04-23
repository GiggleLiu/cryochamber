use super::*;

#[test]
fn test_backward_compat_missing_at_field() {
    // Legacy JSON without the `at` field should deserialize with default empty string.
    let json = r#"[{"id":1,"text":"old item","done":false,"created":"unknown"}]"#;
    let items: Vec<TodoItem> = serde_json::from_str(json).unwrap();
    assert_eq!(items[0].at, "", "Missing at should default to empty string");
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

    let consumed = todos
        .consume_past_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();
    assert_eq!(
        consumed,
        vec![("due task".to_string(), "2026-03-02T10:00".to_string())]
    );

    let display = todos.display().unwrap();
    assert!(display.contains("1. [x] due task (at: 2026-03-02T10:00)"));
    assert!(display.contains("2. [ ] future task (at: 2026-03-02T12:00)"));

    let retry_ids = todos
        .reschedule_consumed(
            &consumed,
            chrono::NaiveDateTime::parse_from_str("2026-03-02T10:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();
    assert_eq!(retry_ids.len(), 1);

    let display = todos.display().unwrap();
    assert!(display.contains("due task (attempt 1) (at: 2026-03-02T10:32)"));

    todos.done(future_id).unwrap();
    todos.remove(retry_ids[0]).unwrap();

    let display = todos.display().unwrap();
    assert!(display.contains("1. [x] due task (at: 2026-03-02T10:00)"));
    assert!(display.contains("2. [x] future task (at: 2026-03-02T12:00)"));
    assert!(!display.contains("attempt 1"));

    assert_eq!(due_id, 1);
}
