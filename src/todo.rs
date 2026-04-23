use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Minute-precision format shared with the daemon scheduler.
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

/// File-backed TODO operations. This is the single entry point for reading and
/// mutating `todo.json` on disk; higher layers should delegate here instead of
/// open-coding load/mutate/save sequences.
#[derive(Debug, Clone)]
pub struct TodoFile {
    path: PathBuf,
}

impl TodoFile {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn items(&self) -> Result<Vec<TodoItem>> {
        load_items(&self.path)
    }

    pub fn display(&self) -> Result<String> {
        let items = load_items(&self.path)?;
        if items.is_empty() {
            return Ok("No todos.".to_string());
        }
        Ok(items
            .iter()
            .map(|item| {
                let check = if item.done { "x" } else { " " };
                format!("{}. [{}] {} (at: {})", item.id, check, item.text, item.at)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn next_wake_time(&self) -> Result<Option<String>> {
        Ok(load_items(&self.path)?
            .iter()
            .filter(|i| !i.done && !i.at.is_empty())
            .map(|i| i.at.as_str())
            .min()
            .map(String::from))
    }

    pub fn next_valid_wake(&self) -> Result<Option<NaiveDateTime>> {
        Ok(load_items(&self.path)?
            .iter()
            .filter(|i| !i.done && !i.at.is_empty())
            .filter_map(|i| {
                let parsed = NaiveDateTime::parse_from_str(&i.at, WAKE_TIME_FMT);
                if parsed.is_err() {
                    eprintln!(
                        "Daemon: Skipping TODO #{} with invalid at value: {:?}",
                        i.id, i.at
                    );
                }
                parsed.ok()
            })
            .min())
    }

    pub fn add(&self, text: String, at: String) -> Result<u32> {
        self.update(|items| {
            if let Some(existing) = items
                .iter()
                .find(|i| !i.done && i.text == text && i.at == at)
            {
                return Ok(existing.id);
            }
            let id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            let created = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            items.push(TodoItem {
                id,
                text,
                done: false,
                at,
                created,
            });
            Ok(id)
        })
    }

    pub fn done(&self, id: u32) -> Result<()> {
        self.update(|items| {
            let item = items
                .iter_mut()
                .find(|i| i.id == id)
                .with_context(|| format!("Todo item {id} not found"))?;
            item.done = true;
            Ok(())
        })
    }

    pub fn remove(&self, id: u32) -> Result<()> {
        self.update(|items| {
            let pos = items
                .iter()
                .position(|i| i.id == id)
                .with_context(|| format!("Todo item {id} not found"))?;
            items.remove(pos);
            Ok(())
        })
    }

    pub fn consume_past_due(&self, now: &NaiveDateTime) -> Result<Vec<(String, String)>> {
        let mut items = load_items(&self.path)?;
        let mut consumed = Vec::new();
        for item in &mut items {
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
        if !consumed.is_empty() {
            save_items(&self.path, &items)?;
        }
        Ok(consumed)
    }

    pub fn reschedule_consumed(
        &self,
        consumed: &[(String, String)],
        now: NaiveDateTime,
    ) -> Result<Vec<u32>> {
        if consumed.is_empty() {
            return Ok(Vec::new());
        }

        self.update(|items| {
            let mut ids = Vec::with_capacity(consumed.len());
            for (text, _) in consumed {
                let (new_text, attempt) = bump_attempt(text);
                let delay_min = retry_delay_minutes(attempt);
                let at = now + chrono::Duration::minutes(delay_min);
                let at_str = at.format(WAKE_TIME_FMT).to_string();

                if let Some(existing) = items
                    .iter()
                    .find(|i| !i.done && i.text == new_text && i.at == at_str)
                {
                    ids.push(existing.id);
                    continue;
                }

                let id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                let created = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                items.push(TodoItem {
                    id,
                    text: new_text,
                    done: false,
                    at: at_str,
                    created,
                });
                ids.push(id);
            }
            Ok(ids)
        })
    }

    fn update<R>(&self, f: impl FnOnce(&mut Vec<TodoItem>) -> Result<R>) -> Result<R> {
        let mut items = load_items(&self.path)?;
        let output = f(&mut items)?;
        save_items(&self.path, &items)?;
        Ok(output)
    }
}

fn load_items(path: &Path) -> Result<Vec<TodoItem>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

fn save_items(path: &Path, items: &[TodoItem]) -> Result<()> {
    let content = serde_json::to_string(items)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(".todo.json.tmp");
    std::fs::write(&tmp, &content).with_context(|| format!("Failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("Failed to rename to {}", path.display()))?;
    Ok(())
}

fn parse_attempt(text: &str) -> (String, u32) {
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

fn bump_attempt(text: &str) -> (String, u32) {
    let (base, prev) = parse_attempt(text);
    let next = prev.saturating_add(1);
    (format!("{base} (attempt {next})"), next)
}

fn retry_delay_minutes(attempt: u32) -> i64 {
    let k = attempt.max(1);
    if k >= 11 {
        return RETRY_DELAY_CAP_MINUTES;
    }
    (1i64 << k).min(RETRY_DELAY_CAP_MINUTES)
}

#[cfg(test)]
#[path = "unit_tests/todo.rs"]
mod tests;
