use assert_cmd::Command;
use cryochamber::todo::{TodoFile, TodoItem};
use std::path::{Path, PathBuf};

fn todo_path(dir: &Path) -> PathBuf {
    dir.join("todo.json")
}

fn todo_file(dir: &Path) -> TodoFile {
    TodoFile::new(todo_path(dir))
}

fn write_todos(path: &Path, items: &[TodoItem]) {
    std::fs::write(path, serde_json::to_string(items).unwrap()).unwrap();
}

#[test]
fn test_load_missing_file_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    assert!(todos.items().unwrap().is_empty());
}

#[test]
fn test_save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());

    todos
        .add("First task".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    todos
        .add("Second task".to_string(), "2026-03-05T14:00".to_string())
        .unwrap();

    let loaded = todos.items().unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].text, "First task");
    assert_eq!(loaded[0].id, 1);
    assert!(!loaded[0].done);
    assert_eq!(loaded[0].at, "2026-03-01T10:00");
    assert_eq!(loaded[1].text, "Second task");
    assert_eq!(loaded[1].id, 2);
    assert_eq!(loaded[1].at, "2026-03-05T14:00");
}

#[test]
fn test_save_is_compact_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = todo_path(dir.path());
    let todos = todo_file(dir.path());

    todos
        .add("Task".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        !content.contains('\n'),
        "JSON should be compact (no line breaks)"
    );
}

#[test]
fn test_add_assigns_incremental_ids() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    let id1 = todos
        .add("A".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    let id2 = todos
        .add("B".to_string(), "2026-03-01T11:00".to_string())
        .unwrap();
    let id3 = todos
        .add("C".to_string(), "2026-03-01T12:00".to_string())
        .unwrap();
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
}

#[test]
fn test_done_marks_item_complete() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    let id = todos
        .add("Task".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    assert!(!todos.items().unwrap()[0].done);
    todos.done(id).unwrap();
    assert!(todos.items().unwrap()[0].done);
}

#[test]
fn test_done_nonexistent_id_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    let result = todos.done(999);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("999"));
}

#[test]
fn test_done_already_done_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    todos
        .add("Task".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    todos.done(1).unwrap();
    assert!(todos.items().unwrap()[0].done);
    // Calling done again should succeed silently
    todos.done(1).unwrap();
    assert!(todos.items().unwrap()[0].done);
}

#[test]
fn test_remove_deletes_item() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    todos
        .add("A".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    let id2 = todos
        .add("B".to_string(), "2026-03-01T11:00".to_string())
        .unwrap();
    todos
        .add("C".to_string(), "2026-03-01T12:00".to_string())
        .unwrap();
    assert_eq!(todos.items().unwrap().len(), 3);
    todos.remove(id2).unwrap();
    let items = todos.items().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].text, "A");
    assert_eq!(items[1].text, "C");
}

#[test]
fn test_remove_nonexistent_id_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    let result = todos.remove(42);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("42"));
}

#[test]
fn test_id_assignment_after_removal() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    todos
        .add("A".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    let id2 = todos
        .add("B".to_string(), "2026-03-01T11:00".to_string())
        .unwrap();
    todos.remove(id2).unwrap();
    // Next ID should be max(existing) + 1 = 2, not 3
    let id3 = todos
        .add("C".to_string(), "2026-03-01T12:00".to_string())
        .unwrap();
    assert_eq!(id3, 2);
}

#[test]
fn test_done_roundtrip_preserves_done_state() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());

    todos
        .add("Task".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    todos.done(1).unwrap();

    let loaded = todos.items().unwrap();
    assert!(loaded[0].done);
}

#[test]
fn test_load_empty_list_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = todo_path(dir.path());
    write_todos(&path, &[]);
    assert!(todo_file(dir.path()).items().unwrap().is_empty());
}

#[test]
fn test_id_auto_increment_after_remove() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    todos
        .add("A".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    todos
        .add("B".to_string(), "2026-03-01T11:00".to_string())
        .unwrap();
    todos.remove(1).unwrap(); // remove A (id=1)
    let id = todos
        .add("C".to_string(), "2026-03-01T12:00".to_string())
        .unwrap();
    assert_eq!(id, 3, "ID should be max(existing)+1, not reuse removed IDs");
}

#[test]
fn test_display_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let todos = todo_file(dir.path());
    assert_eq!(todos.display().unwrap(), "No todos.");

    todos
        .add("First".to_string(), "2026-03-01T10:00".to_string())
        .unwrap();
    todos
        .add("Second".to_string(), "2026-03-05T14:00".to_string())
        .unwrap();
    todos
        .add("Claimed".to_string(), "2026-03-01T09:00".to_string())
        .unwrap();
    todos.done(1).unwrap();
    todos
        .claim_due(
            &chrono::NaiveDateTime::parse_from_str("2026-03-01T09:30", "%Y-%m-%dT%H:%M").unwrap(),
        )
        .unwrap();

    let output = todos.display().unwrap();
    assert!(output.starts_with("1. [x] First (at: 2026-03-01T10:00)\n"));
    assert!(output.contains("2. [ ] Second (at: 2026-03-05T14:00)"));
    assert!(output.contains("3. [~] Claimed (at: 2026-03-01T09:00)"));
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
fn test_cli_todo_pop_is_not_a_command() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["todo", "pop"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("unrecognized subcommand"));
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
