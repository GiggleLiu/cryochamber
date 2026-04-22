use super::*;

#[test]
fn todo_checkmark_marks_completed_items() {
    assert_eq!(todo_checkmark(true), "x");
}

#[test]
fn todo_checkmark_marks_open_items_as_blank() {
    assert_eq!(todo_checkmark(false), " ");
}

#[test]
fn test_next_wake_time_picks_earliest_pending() {
    let mut list = TodoList::new();
    list.add("later task".into(), "2026-03-02T16:00".into());
    list.add("earlier task".into(), "2026-03-02T14:00".into());
    let wake = list.next_wake_time();
    assert_eq!(wake, Some("2026-03-02T14:00"));
}

#[test]
fn test_next_wake_time_skips_done_items() {
    let mut list = TodoList::new();
    let id = list.add("done task".into(), "2026-03-02T10:00".into());
    list.done(id).unwrap();
    list.add("pending task".into(), "2026-03-02T16:00".into());
    let wake = list.next_wake_time();
    assert_eq!(wake, Some("2026-03-02T16:00"));
}

#[test]
fn test_next_wake_time_none_when_all_done() {
    let mut list = TodoList::new();
    let id = list.add("task".into(), "2026-03-02T10:00".into());
    list.done(id).unwrap();
    assert!(list.next_wake_time().is_none());
}

#[test]
fn test_next_wake_time_none_when_empty() {
    let list = TodoList::new();
    assert!(list.next_wake_time().is_none());
}

#[test]
fn test_backward_compat_missing_at_field() {
    // Legacy JSON without the `at` field should deserialize with default empty string
    let json = r#"[{"id":1,"text":"old item","done":false,"created":"unknown"}]"#;
    let items: Vec<TodoItem> = serde_json::from_str(json).unwrap();
    assert_eq!(items[0].at, "", "Missing at should default to empty string");
}

#[test]
fn test_add_dedup_returns_existing_id_for_open_duplicate() {
    let mut list = TodoList::new();
    let first = list.add("[internal] heartbeat".into(), "2026-03-02T21:01".into());
    let second = list.add("[internal] heartbeat".into(), "2026-03-02T21:01".into());
    assert_eq!(first, second);
    assert_eq!(list.items().len(), 1);
}

#[test]
fn test_add_dedup_creates_new_when_existing_is_done() {
    let mut list = TodoList::new();
    let first = list.add("[internal] heartbeat".into(), "2026-03-02T21:01".into());
    list.done(first).unwrap();
    let second = list.add("[internal] heartbeat".into(), "2026-03-02T21:01".into());
    assert_ne!(first, second);
    assert_eq!(list.items().len(), 2);
}

#[test]
fn test_add_dedup_creates_new_when_at_differs() {
    let mut list = TodoList::new();
    let first = list.add("[internal] heartbeat".into(), "2026-03-02T21:01".into());
    let second = list.add("[internal] heartbeat".into(), "2026-03-02T22:00".into());
    assert_ne!(first, second);
    assert_eq!(list.items().len(), 2);
}

#[test]
fn test_add_dedup_creates_new_when_text_differs() {
    let mut list = TodoList::new();
    let first = list.add("[internal] heartbeat".into(), "2026-03-02T21:01".into());
    let second = list.add("call Alice".into(), "2026-03-02T21:01".into());
    assert_ne!(first, second);
    assert_eq!(list.items().len(), 2);
}

#[test]
fn test_next_wake_time_skips_empty_at() {
    let mut list = TodoList::new();
    // Simulate a legacy item with empty `at`
    list.items.push(TodoItem {
        id: 1,
        text: "legacy".into(),
        done: false,
        at: "".into(),
        created: "unknown".into(),
    });
    list.add("scheduled".into(), "2026-03-02T14:00".into());
    // next_wake_time should skip empty `at` and return the scheduled item
    let wake = list.next_wake_time();
    assert_eq!(wake, Some("2026-03-02T14:00"));
}
