use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single todo item with an ID, text, optional scheduled time, and completion status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u32,
    pub text: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub created: String,
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
    pub fn add(&mut self, text: String, at: Option<String>) -> u32 {
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
                let at_suffix = match &item.at {
                    Some(at) => format!(" (at: {at})"),
                    None => String::new(),
                };
                format!("{}. [{}] {}{}", item.id, check, item.text, at_suffix)
            })
            .collect::<Vec<_>>()
            .join("\n")
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
