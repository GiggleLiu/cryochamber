use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single todo item with an ID, text, scheduled time, and completion status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u32,
    pub text: String,
    pub done: bool,
    #[serde(default)]
    pub at: String,
    #[serde(default = "default_created")]
    pub created: String,
}

fn default_created() -> String {
    "unknown".to_string()
}

/// A list of todo items with load/save persistence.
#[derive(Debug, Default)]
pub struct TodoList {
    items: Vec<TodoItem>,
}

impl TodoList {
    /// Create a new empty todo list.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Get a reference to all items.
    pub fn items(&self) -> &[TodoItem] {
        &self.items
    }

    /// Load from file. Returns empty list if file doesn't exist.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let items: Vec<TodoItem> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(Self { items })
    }

    /// Save to file atomically (write to temp, rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string(&self.items)?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let tmp = dir.join(".todo.json.tmp");
        std::fs::write(&tmp, &content)
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("Failed to rename to {}", path.display()))?;
        Ok(())
    }

    /// Add item. Returns the new item's ID.
    pub fn add(&mut self, text: String, at: String) -> u32 {
        let id = self.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        let created = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.items.push(TodoItem {
            id,
            text,
            done: false,
            at,
            created,
        });
        id
    }

    /// Mark item as done. Returns error if ID not found.
    pub fn done(&mut self, id: u32) -> Result<()> {
        let item = self
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .with_context(|| format!("Todo item {id} not found"))?;
        item.done = true;
        Ok(())
    }

    /// Format the list for display.
    pub fn display(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".to_string();
        }
        self.items
            .iter()
            .map(|item| {
                let check = if item.done { "x" } else { " " };
                format!("{}. [{}] {} (at: {})", item.id, check, item.text, item.at)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Return the earliest `at` among pending (not done) items, if any.
    /// Skips items with empty `at` (e.g. legacy items missing the field).
    pub fn next_wake_time(&self) -> Option<&str> {
        self.items
            .iter()
            .filter(|i| !i.done && !i.at.is_empty())
            .map(|i| i.at.as_str())
            .min()
    }

    /// Remove item. Returns error if ID not found.
    pub fn remove(&mut self, id: u32) -> Result<()> {
        let pos = self
            .items
            .iter()
            .position(|i| i.id == id)
            .with_context(|| format!("Todo item {id} not found"))?;
        self.items.remove(pos);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
