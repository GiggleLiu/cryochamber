use assert_cmd::Command;
use cryochamber::todo::TodoList;

#[test]
fn test_load_missing_file_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let list = TodoList::load(&path).unwrap();
    assert!(list.items().is_empty());
}

#[test]
fn test_save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");

    let mut list = TodoList::new();
    list.add("First task".to_string(), "2026-03-01T10:00".to_string());
    list.add("Second task".to_string(), "2026-03-05T14:00".to_string());
    list.save(&path).unwrap();

    let loaded = TodoList::load(&path).unwrap();
    assert_eq!(loaded.items().len(), 2);
    assert_eq!(loaded.items()[0].text, "First task");
    assert_eq!(loaded.items()[0].id, 1);
    assert!(!loaded.items()[0].done);
    assert_eq!(loaded.items()[0].at, "2026-03-01T10:00");
    assert_eq!(loaded.items()[1].text, "Second task");
    assert_eq!(loaded.items()[1].id, 2);
    assert_eq!(loaded.items()[1].at, "2026-03-05T14:00");
}

#[test]
fn test_save_is_compact_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");

    let mut list = TodoList::new();
    list.add("Task".to_string(), "2026-03-01T10:00".to_string());
    list.save(&path).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        !content.contains('\n'),
        "JSON should be compact (no line breaks)"
    );
}

#[test]
fn test_add_assigns_incremental_ids() {
    let mut list = TodoList::new();
    let id1 = list.add("A".to_string(), "2026-03-01T10:00".to_string());
    let id2 = list.add("B".to_string(), "2026-03-01T11:00".to_string());
    let id3 = list.add("C".to_string(), "2026-03-01T12:00".to_string());
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
}

#[test]
fn test_done_marks_item_complete() {
    let mut list = TodoList::new();
    let id = list.add("Task".to_string(), "2026-03-01T10:00".to_string());
    assert!(!list.items()[0].done);
    list.done(id).unwrap();
    assert!(list.items()[0].done);
}

#[test]
fn test_done_nonexistent_id_returns_error() {
    let mut list = TodoList::new();
    let result = list.done(999);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("999"));
}

#[test]
fn test_done_already_done_is_idempotent() {
    let mut list = TodoList::new();
    list.add("Task".to_string(), "2026-03-01T10:00".to_string());
    list.done(1).unwrap();
    assert!(list.items()[0].done);
    // Calling done again should succeed silently
    list.done(1).unwrap();
    assert!(list.items()[0].done);
}

#[test]
fn test_remove_deletes_item() {
    let mut list = TodoList::new();
    list.add("A".to_string(), "2026-03-01T10:00".to_string());
    let id2 = list.add("B".to_string(), "2026-03-01T11:00".to_string());
    list.add("C".to_string(), "2026-03-01T12:00".to_string());
    assert_eq!(list.items().len(), 3);
    list.remove(id2).unwrap();
    assert_eq!(list.items().len(), 2);
    assert_eq!(list.items()[0].text, "A");
    assert_eq!(list.items()[1].text, "C");
}

#[test]
fn test_remove_nonexistent_id_returns_error() {
    let mut list = TodoList::new();
    let result = list.remove(42);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("42"));
}

#[test]
fn test_id_assignment_after_removal() {
    let mut list = TodoList::new();
    list.add("A".to_string(), "2026-03-01T10:00".to_string());
    let id2 = list.add("B".to_string(), "2026-03-01T11:00".to_string());
    list.remove(id2).unwrap();
    // Next ID should be max(existing) + 1 = 2, not 3
    let id3 = list.add("C".to_string(), "2026-03-01T12:00".to_string());
    assert_eq!(id3, 2);
}

#[test]
fn test_done_roundtrip_preserves_done_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");

    let mut list = TodoList::new();
    list.add("Task".to_string(), "2026-03-01T10:00".to_string());
    list.done(1).unwrap();
    list.save(&path).unwrap();

    let loaded = TodoList::load(&path).unwrap();
    assert!(loaded.items()[0].done);
}

#[test]
fn test_load_empty_list_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");

    let list = TodoList::new();
    list.save(&path).unwrap();

    let loaded = TodoList::load(&path).unwrap();
    assert!(loaded.items().is_empty());
}

#[test]
fn test_id_auto_increment_after_remove() {
    let mut list = TodoList::new();
    list.add("A".to_string(), "2026-03-01T10:00".to_string());
    list.add("B".to_string(), "2026-03-01T11:00".to_string());
    list.remove(1).unwrap(); // remove A (id=1)
    let id = list.add("C".to_string(), "2026-03-01T12:00".to_string());
    assert_eq!(id, 3, "ID should be max(existing)+1, not reuse removed IDs");
}

#[test]
fn test_display_formatting() {
    let mut list = TodoList::new();
    assert_eq!(list.display(), "No todos.");

    list.add("First".to_string(), "2026-03-01T10:00".to_string());
    list.add("Second".to_string(), "2026-03-05T14:00".to_string());
    list.done(1).unwrap();

    let output = list.display();
    assert!(output.starts_with("1. [x] First (at: 2026-03-01T10:00)\n"));
    assert!(output.contains("2. [ ] Second (at: 2026-03-05T14:00)"));
}

fn agent_cmd() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo-agent").unwrap()
}

// CLI TODO tests — since TODO ops now go through the daemon socket,
// these tests verify the no-daemon (connection error) behavior.

#[test]
fn test_cli_todo_add_requires_at() {
    let dir = tempfile::tempdir().unwrap();
    // --at is now required; omitting it should fail at CLI parse level
    agent_cmd()
        .args(["todo", "add", "Submit paper"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("--at"));
}

#[test]
fn test_cli_todo_add_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["todo", "add", "Submit paper", "--at", "2026-03-05T14:00"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("Cannot connect"));
}

#[test]
fn test_cli_todo_list_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["todo", "list"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("Cannot connect"));
}

#[test]
fn test_cli_todo_done_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["todo", "done", "1"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("Cannot connect"));
}

#[test]
fn test_cli_todo_remove_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["todo", "remove", "1"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("Cannot connect"));
}
