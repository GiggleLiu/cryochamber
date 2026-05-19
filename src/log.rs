// src/log.rs
use anyhow::Result;
use chrono::NaiveDateTime;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn log_path(dir: &Path) -> PathBuf {
    dir.join("cryo.log")
}

pub fn agent_log_path(dir: &Path) -> PathBuf {
    dir.join("cryo-agent.log")
}

pub const SESSION_START: &str = "--- CRYO SESSION";
pub const SESSION_END: &str = "--- CRYO END ---";

pub fn read_latest_session(log_path: &Path) -> Result<Option<String>> {
    if !log_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(log_path)?;
    if contents.trim().is_empty() {
        return Ok(None);
    }

    let last_start = contents.rfind(SESSION_START);
    let last_end = contents.rfind(SESSION_END);

    match (last_start, last_end) {
        (Some(start), Some(end)) if end > start => {
            let session_text = &contents[start..end + SESSION_END.len()];
            Ok(Some(session_text.to_string()))
        }
        _ => Ok(None),
    }
}

/// Read the most recent session from cryo.log, whether or not it has finished.
/// Returns from the last `SESSION_START` to EOF.
pub fn read_current_session(log_path: &Path) -> Result<Option<String>> {
    if !log_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(log_path)?;
    if contents.trim().is_empty() {
        return Ok(None);
    }

    match contents.rfind(SESSION_START) {
        Some(start) => Ok(Some(contents[start..].to_string())),
        None => Ok(None),
    }
}

/// Read the last `n` sessions from `cryo.log` as a single string, preserving
/// their `--- CRYO SESSION … ---` / `--- CRYO END ---` delimiters. If fewer
/// than `n` sessions exist, returns everything from the first session onwards.
/// Returns `None` for a missing or empty log, or `n == 0`.
pub fn read_recent_sessions(log_path: &Path, n: usize) -> Result<Option<String>> {
    if !log_path.exists() || n == 0 {
        return Ok(None);
    }
    let contents = fs::read_to_string(log_path)?;
    if contents.trim().is_empty() {
        return Ok(None);
    }
    let indices: Vec<usize> = contents
        .match_indices(SESSION_START)
        .map(|(i, _)| i)
        .collect();
    if indices.is_empty() {
        return Ok(None);
    }
    let start = indices[indices.len().saturating_sub(n)];
    Ok(Some(contents[start..].to_string()))
}

pub fn session_count(log_path: &Path) -> Result<u32> {
    if !log_path.exists() {
        return Ok(0);
    }
    let contents = fs::read_to_string(log_path)?;
    Ok(contents.matches(SESSION_START).count() as u32)
}

/// Extract the most recent wake time from the log.
/// Scans the entire log backward so the value survives session restarts.
/// Returns the raw time string (e.g. "2026-03-01T09:00").
pub fn parse_latest_session_wake(log_path: &Path) -> Result<Option<String>> {
    if !log_path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(log_path)?;
    // Lines look like: [HH:MM:SS] hibernate: wake=2026-03-01T09:00, exit=0, ...
    for line in contents.lines().rev() {
        if let Some(pos) = line.find("hibernate: wake=") {
            let after = pos + "hibernate: wake=".len();
            if let Some(rest) = line.get(after..) {
                let wake = rest.split(',').next().unwrap_or("").trim();
                if !wake.is_empty() {
                    return Ok(Some(wake.to_string()));
                }
            }
        }
    }
    Ok(None)
}

/// Extract the plan-complete summary from the latest session, if any.
/// Matches lines like `[HH:MM:SS] hibernate: plan complete, exit=0, summary="..."`.
/// Returns `None` if the latest session did not end with plan completion.
pub fn parse_latest_session_plan_complete(log_path: &Path) -> Result<Option<String>> {
    let session = match read_current_session(log_path)? {
        Some(s) => s,
        None => return Ok(None),
    };
    for line in session.lines() {
        if !line.contains("hibernate: plan complete") {
            continue;
        }
        let summary = line
            .find("summary=\"")
            .and_then(|pos| line.get(pos + "summary=\"".len()..))
            .and_then(|rest| rest.rfind('"').map(|end| rest[..end].to_string()))
            .unwrap_or_default();
        return Ok(Some(summary));
    }
    Ok(None)
}

/// Extract the hibernate summary from the current session, if any.
/// Matches both scheduled and plan-complete hibernate lines with
/// `summary="..."`.
pub fn parse_latest_session_summary(log_path: &Path) -> Result<Option<String>> {
    let session = match read_current_session(log_path)? {
        Some(s) => s,
        None => return Ok(None),
    };
    for line in session.lines().rev() {
        if !line.contains("hibernate:") {
            continue;
        }
        let summary = line
            .find("summary=\"")
            .and_then(|pos| line.get(pos + "summary=\"".len()..))
            .and_then(|rest| rest.rfind('"').map(|end| rest[..end].to_string()));
        if summary.is_some() {
            return Ok(summary);
        }
    }
    Ok(None)
}

/// Extract the task line from the current session in cryo.log.
pub fn parse_latest_session_task(log_path: &Path) -> Result<Option<String>> {
    let session = match read_current_session(log_path)? {
        Some(s) => s,
        None => return Ok(None),
    };
    for line in session.lines() {
        if let Some(task) = line.strip_prefix("task: ") {
            return Ok(Some(task.to_string()));
        }
    }
    Ok(None)
}

/// Outcome of a completed session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOutcome {
    Success,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Copy)]
struct SessionOutcomeRule {
    markers: &'static [&'static str],
    outcome: SessionOutcome,
}

impl SessionOutcomeRule {
    fn matches(&self, block: &str) -> bool {
        self.markers.iter().any(|marker| block.contains(marker))
    }
}

const SESSION_OUTCOME_RULES: &[SessionOutcomeRule] = &[
    SessionOutcomeRule {
        markers: &["--- CRYO INTERRUPTED ---"],
        outcome: SessionOutcome::Interrupted,
    },
    SessionOutcomeRule {
        markers: &["quick exit detected", "agent exited without hibernate"],
        outcome: SessionOutcome::Failed,
    },
    SessionOutcomeRule {
        markers: &["hibernate:", "agent exited (code 0)"],
        outcome: SessionOutcome::Success,
    },
];

fn classify_session_outcome(block: &str) -> SessionOutcome {
    SESSION_OUTCOME_RULES
        .iter()
        .find(|rule| rule.matches(block))
        .map(|rule| rule.outcome)
        .unwrap_or(SessionOutcome::Failed)
}

/// Summary of a single session extracted from cryo.log.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_number: u32,
    pub timestamp: NaiveDateTime,
    pub outcome: SessionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DailyDigest {
    pub date: String,
    pub total_sessions: usize,
    pub failed_sessions: usize,
    pub latest_session: u32,
}

#[derive(Debug, Default)]
struct DailyDigestAccumulator {
    total_sessions: usize,
    failed_sessions: usize,
    latest_session: u32,
}

/// Summarize recent session activity by the UTC date recorded in `cryo.log`.
/// Results are newest day first. Missing or empty logs return an empty list.
pub fn daily_digests(log_path: &Path, max_days: usize) -> Result<Vec<DailyDigest>> {
    if max_days == 0 {
        return Ok(Vec::new());
    }

    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let sessions = parse_sessions_since(log_path, epoch)?;
    let mut by_date: BTreeMap<String, DailyDigestAccumulator> = BTreeMap::new();

    for session in sessions {
        let date = session.timestamp.date().format("%Y-%m-%d").to_string();
        let entry = by_date.entry(date).or_default();
        entry.total_sessions += 1;
        if matches!(
            session.outcome,
            SessionOutcome::Failed | SessionOutcome::Interrupted
        ) {
            entry.failed_sessions += 1;
        }
        entry.latest_session = entry.latest_session.max(session.session_number);
    }

    Ok(by_date
        .into_iter()
        .rev()
        .take(max_days)
        .map(|(date, acc)| DailyDigest {
            date,
            total_sessions: acc.total_sessions,
            failed_sessions: acc.failed_sessions,
            latest_session: acc.latest_session,
        })
        .collect())
}

/// Parse all sessions from `cryo.log` whose timestamp is >= `since`.
/// Returns a vec of session summaries sorted chronologically.
pub fn parse_sessions_since(log_path: &Path, since: NaiveDateTime) -> Result<Vec<SessionSummary>> {
    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(log_path)?;
    let mut summaries = Vec::new();

    // Split into session blocks by finding SESSION_START markers
    let starts: Vec<usize> = contents
        .match_indices(SESSION_START)
        .map(|(i, _)| i)
        .collect();

    for (idx, &start) in starts.iter().enumerate() {
        let end = if idx + 1 < starts.len() {
            starts[idx + 1]
        } else {
            contents.len()
        };
        let block = &contents[start..end];

        // Parse header: "--- CRYO SESSION N | 2026-02-28T14:30:45Z ---"
        let header_line = block.lines().next().unwrap_or("");
        let (session_number, timestamp) = match parse_session_header(header_line) {
            Some(v) => v,
            None => continue,
        };

        if timestamp < since {
            continue;
        }

        summaries.push(SessionSummary {
            session_number,
            timestamp,
            outcome: classify_session_outcome(block),
        });
    }

    Ok(summaries)
}

/// Parse a session header line into (session_number, timestamp).
fn parse_session_header(line: &str) -> Option<(u32, NaiveDateTime)> {
    // "--- CRYO SESSION 3 | 2026-02-28T14:30:45Z ---"
    let after_prefix = line.strip_prefix(SESSION_START)?.trim_start();
    let parts: Vec<&str> = after_prefix.splitn(2, '|').collect();
    if parts.len() != 2 {
        return None;
    }
    let session_number: u32 = parts[0].trim().parse().ok()?;
    let ts_str = parts[1].trim().trim_end_matches("---").trim();
    let timestamp = chrono::NaiveDateTime::parse_from_str(ts_str, "%Y-%m-%dT%H:%M:%SZ").ok()?;
    Some((session_number, timestamp))
}

/// Event-based session logger. Only cryo writes to this log.
pub struct EventLogger {
    file: fs::File,
    finished: bool,
}

impl EventLogger {
    /// Begin a new session in the event log.
    pub fn begin(
        log_path: &Path,
        session_number: u32,
        task: &str,
        agent_cmd: &str,
        inbox_filenames: &[String],
    ) -> Result<Self, anyhow::Error> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;

        let now = chrono::Utc::now();
        writeln!(
            file,
            "--- CRYO SESSION {session_number} | {} ---",
            now.format("%Y-%m-%dT%H:%M:%SZ")
        )?;
        writeln!(file, "task: {task}")?;
        writeln!(file, "agent: {agent_cmd}")?;

        if inbox_filenames.is_empty() {
            writeln!(file, "inbox: 0 messages")?;
        } else {
            writeln!(
                file,
                "inbox: {} messages ({})",
                inbox_filenames.len(),
                inbox_filenames.join(", ")
            )?;
        }

        file.flush()?;
        Ok(Self {
            file,
            finished: false,
        })
    }

    /// Log a timestamped event.
    pub fn log_event(&mut self, event: &str) -> Result<(), anyhow::Error> {
        let now = chrono::Utc::now();
        writeln!(self.file, "[{}] {event}", now.format("%H:%M:%S"))?;
        self.file.flush()?;
        Ok(())
    }

    /// Finish the session with a final event.
    pub fn finish(mut self, final_event: &str) -> Result<(), anyhow::Error> {
        self.log_event(final_event)?;
        writeln!(self.file, "{SESSION_END}")?;
        self.file.flush()?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for EventLogger {
    fn drop(&mut self) {
        if !self.finished {
            let _ = writeln!(self.file, "--- CRYO INTERRUPTED ---");
        }
    }
}

#[cfg(test)]
#[path = "unit_tests/log.rs"]
mod tests;
