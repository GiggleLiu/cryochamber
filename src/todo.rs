use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Canonical minute-precision format used when Cryochamber writes TODO times.
const WAKE_TIME_FMT: &str = "%Y-%m-%dT%H:%M";

const WAKE_TIME_INPUT_FORMATS: &[&str] = &[WAKE_TIME_FMT, "%Y-%m-%dT%H:%M:%S"];

/// Maximum per-attempt reschedule delay. Beyond this, exponential backoff is
/// clamped to one day so a persistently failing TODO still polls once a day.
const RETRY_DELAY_CAP_MINUTES: i64 = 24 * 60;

/// A single todo item with an ID, text, scheduled time, and lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u32,
    pub text: String,
    pub done: bool,
    #[serde(default)]
    pub claimed: bool,
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

pub fn parse_wake_time(input: &str) -> std::result::Result<NaiveDateTime, chrono::ParseError> {
    let mut last_error = None;
    for fmt in WAKE_TIME_INPUT_FORMATS {
        match NaiveDateTime::parse_from_str(input, fmt) {
            Ok(parsed) => return Ok(parsed),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.expect("wake time input format list must not be empty"))
}

/// User-facing description of the wake-time input grammar shared by
/// `cryo-agent time` and `cryo-agent todo add --at`.
pub fn wake_input_usage_error(got: &str) -> String {
    format!(
        "unrecognized time expression {got:?}.\n\
         Accepted forms:\n  \
           +30 minutes            # relative offset (minutes|hours|days|weeks)\n  \
           2026-04-25T10:00       # absolute ISO8601 (also with :SS, a space\n  \
                                  # separator, or date-only = midnight)\n\
         Run `cryo-agent time` for the current time. For natural expressions like\n\
         \"tomorrow 9am\", compute the absolute timestamp yourself and pass it directly."
    )
}

/// Normalize any accepted wake-time input to the canonical `%Y-%m-%dT%H:%M`
/// form: a relative offset resolved against `now`, or an absolute ISO8601
/// timestamp. Anything else fails with the accepted-forms usage error.
pub fn normalize_wake_input(input: &str, now: NaiveDateTime) -> anyhow::Result<String> {
    let s = input.trim();
    if looks_like_iso_date(s) {
        return parse_iso_timestamp(s);
    }
    let offset = parse_relative_offset(s)?;
    Ok((now + offset).format(WAKE_TIME_FMT).to_string())
}

/// Heuristic: input starts with `YYYY-MM-DD` → try ISO8601.
fn looks_like_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

/// Parse an ISO8601-ish absolute timestamp and return it normalized to `%Y-%m-%dT%H:%M`.
pub fn parse_iso_timestamp(s: &str) -> anyhow::Result<String> {
    let dt_formats = [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
    ];
    for fmt in &dt_formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt.format(WAKE_TIME_FMT).to_string());
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date.and_hms_opt(0, 0, 0).unwrap();
        return Ok(dt.format(WAKE_TIME_FMT).to_string());
    }
    anyhow::bail!("{}", wake_input_usage_error(s))
}

/// Parse "+N minutes|hours|days|weeks" (the `+` is optional).
pub fn parse_relative_offset(s: &str) -> anyhow::Result<chrono::Duration> {
    let rel = s.trim_start_matches('+');
    let parts: Vec<&str> = rel.splitn(2, ' ').collect();
    if parts.len() != 2 {
        anyhow::bail!("{}", wake_input_usage_error(s));
    }
    let n: i64 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("{}", wake_input_usage_error(s)))?;
    // Reject negative offsets: a past `at` time becomes an immediately-due TODO
    // (spurious instant wake). Only non-negative relative offsets are accepted.
    if n < 0 {
        anyhow::bail!("{}", wake_input_usage_error(s));
    }
    let unit = parts[1].trim_end_matches('s');
    match unit {
        "minute" | "min" => Ok(chrono::Duration::minutes(n)),
        "hour" | "hr" => Ok(chrono::Duration::hours(n)),
        "day" => Ok(chrono::Duration::days(n)),
        "week" => Ok(chrono::Duration::weeks(n)),
        _ => anyhow::bail!("{}", wake_input_usage_error(s)),
    }
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
                let check = if item.done {
                    "x"
                } else if item.claimed {
                    "~"
                } else {
                    " "
                };
                format!("{}. [{}] {} (at: {})", item.id, check, item.text, item.at)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn next_wake_time(&self) -> Result<Option<String>> {
        Ok(load_items(&self.path)?
            .iter()
            .filter(|i| !i.done && !i.claimed && !i.at.is_empty())
            .map(|i| i.at.as_str())
            .min()
            .map(String::from))
    }

    pub fn next_valid_wake(&self) -> Result<Option<NaiveDateTime>> {
        Ok(load_items(&self.path)?
            .iter()
            .filter(|i| !i.done && !i.claimed && !i.at.is_empty())
            .filter_map(|i| {
                let parsed = parse_wake_time(&i.at);
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
                .find(|i| !i.done && !i.claimed && i.text == text && i.at == at)
            {
                return Ok(existing.id);
            }
            let id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            let created = chrono::Local::now()
                .naive_local()
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string();
            items.push(TodoItem {
                id,
                text,
                done: false,
                claimed: false,
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
            item.claimed = false;
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

    pub fn claim_due(&self, now: &NaiveDateTime) -> Result<Vec<TodoItem>> {
        let mut items = load_items(&self.path)?;
        let mut claimed = Vec::new();
        for item in &mut items {
            if item.done || item.claimed || item.at.is_empty() {
                continue;
            }
            if let Ok(at) = parse_wake_time(&item.at) {
                if at <= *now {
                    item.claimed = true;
                    claimed.push(item.clone());
                }
            }
        }
        if !claimed.is_empty() {
            save_items(&self.path, &items)?;
        }
        Ok(claimed)
    }

    pub fn complete_claimed(&self) -> Result<Vec<TodoItem>> {
        self.update(|items| {
            let mut completed = Vec::new();
            for item in items.iter_mut() {
                if item.done || !item.claimed {
                    continue;
                }
                item.done = true;
                item.claimed = false;
                completed.push(item.clone());
            }
            Ok(completed)
        })
    }

    pub fn reschedule_claimed_after_crash(&self, now: NaiveDateTime) -> Result<Vec<u32>> {
        self.update(|items| {
            let mut consumed = Vec::new();
            for item in items.iter_mut() {
                if item.done || !item.claimed {
                    continue;
                }
                consumed.push((item.text.clone(), item.at.clone()));
                item.done = true;
                item.claimed = false;
            }

            let mut ids = Vec::with_capacity(consumed.len());
            for (text, _) in consumed {
                let (new_text, attempt) = bump_attempt(&text);
                let delay_min = retry_delay_minutes(attempt);
                let at = now + chrono::Duration::minutes(delay_min);
                let at_str = at.format(WAKE_TIME_FMT).to_string();

                if let Some(existing) = items
                    .iter()
                    .find(|i| !i.done && !i.claimed && i.text == new_text && i.at == at_str)
                {
                    ids.push(existing.id);
                    continue;
                }

                let id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                let created = chrono::Local::now()
                    .naive_local()
                    .format("%Y-%m-%dT%H:%M:%S")
                    .to_string();
                items.push(TodoItem {
                    id,
                    text: new_text,
                    done: false,
                    claimed: false,
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
    if content.trim().is_empty() {
        // File exists but is empty — likely caught mid-write (truncate-then-write
        // race). Treat as no items rather than hard-erroring, which would
        // otherwise silently disable all scheduling and retry. Mirrors
        // `state::load_state`.
        return Ok(Vec::new());
    }
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
