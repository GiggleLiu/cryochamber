use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Minute-precision format shared with the daemon scheduler. Kept private so
/// `TodoList` owns all time-string parsing in one place.
const WAKE_TIME_FMT: &str = "%Y-%m-%dT%H:%M";

/// Maximum per-attempt reschedule delay. Beyond this, exponential backoff is
/// clamped to one day so a persistently failing TODO still polls once a day.
const RETRY_DELAY_CAP_MINUTES: i64 = 24 * 60;

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

    /// Add item. Returns the new item's ID, or the id of an existing open item
    /// with identical text + at (dedup).
    pub fn add(&mut self, text: String, at: String) -> u32 {
        if let Some(existing) = self
            .items
            .iter()
            .find(|i| !i.done && i.text == text && i.at == at)
        {
            return existing.id;
        }
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
                let check = todo_checkmark(item.done);
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

    /// Mark every pending TODO whose `at` time is <= `now` as done and
    /// return `(text, at)` for each. Items with invalid / empty `at` are
    /// skipped. Call this at session wake so the agent does not re-react
    /// to the same TODO on the next session and the scheduler does not
    /// re-fire the wake immediately. The returned entries let the caller
    /// re-inject them with an attempt bump if the session crashes.
    pub fn consume_past_due(&mut self, now: &NaiveDateTime) -> Vec<(String, String)> {
        let mut consumed = Vec::new();
        for item in self.items.iter_mut() {
            if item.done || item.at.is_empty() {
                continue;
            }
            if let Ok(at) = NaiveDateTime::parse_from_str(&item.at, WAKE_TIME_FMT) {
                if at <= *now {
                    consumed.push((item.text.clone(), item.at.clone()));
                    item.done = true;
                }
            }
        }
        consumed
    }
}

/// Strip a trailing ` (attempt N)` suffix. Returns (base text, attempt
/// number). Text without the suffix is treated as attempt 0.
pub fn parse_attempt(text: &str) -> (String, u32) {
    if let Some(open) = text.rfind(" (attempt ") {
        let rest = &text[open + " (attempt ".len()..];
        if let Some(inner) = rest.strip_suffix(')') {
            if let Ok(n) = inner.parse::<u32>() {
                return (text[..open].to_string(), n);
            }
        }
    }
    (text.to_string(), 0)
}

/// Render attempt-bumped text. `parse_attempt(text).1 + 1` is the new
/// attempt number; `"base" -> "base (attempt 1)"`,
/// `"base (attempt 1)" -> "base (attempt 2)"`.
pub fn bump_attempt(text: &str) -> (String, u32) {
    let (base, prev) = parse_attempt(text);
    let next = prev.saturating_add(1);
    (format!("{base} (attempt {next})"), next)
}

/// Reschedule delay in minutes for attempt `k` (1-indexed). `2^k`,
/// clamped at `RETRY_DELAY_CAP_MINUTES` (1 day). `k == 0` is treated as
/// `k == 1` (minimum delay) to keep the caller's arithmetic simple when
/// the upstream attempt counter has not been bumped yet.
pub fn retry_delay_minutes(attempt: u32) -> i64 {
    let k = attempt.max(1);
    if k >= 11 {
        return RETRY_DELAY_CAP_MINUTES;
    }
    (1i64 << k).min(RETRY_DELAY_CAP_MINUTES)
}

/// Re-inject consumed TODOs after a crashed session. Each `(text, _at)`
/// becomes a fresh item whose text is `bump_attempt(text)` and whose
/// `at` is `now + retry_delay_minutes(attempt)`. Returns the IDs of the
/// added items.
pub fn reschedule_consumed(
    list: &mut TodoList,
    consumed: &[(String, String)],
    now: NaiveDateTime,
) -> Vec<u32> {
    let mut ids = Vec::new();
    for (text, _) in consumed {
        let (new_text, attempt) = bump_attempt(text);
        let delay_min = retry_delay_minutes(attempt);
        let at = now + chrono::Duration::minutes(delay_min);
        let at_str = at.format(WAKE_TIME_FMT).to_string();
        ids.push(list.add(new_text, at_str));
    }
    ids
}

fn todo_checkmark(done: bool) -> &'static str {
    match done {
        true => "x",
        false => " ",
    }
}

#[cfg(test)]
#[path = "unit_tests/todo.rs"]
mod tests;
