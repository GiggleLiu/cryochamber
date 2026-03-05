// src/log.rs
use anyhow::Result;
use chrono::NaiveDateTime;
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

pub fn session_count(log_path: &Path) -> Result<u32> {
    if !log_path.exists() {
        return Ok(0);
    }
    let contents = fs::read_to_string(log_path)?;
    Ok(contents.matches(SESSION_START).count() as u32)
}

/// Extract `note: "..."` lines from the most recent session that has notes.
/// Scans backward through sessions so a restart doesn't hide previous notes.
pub fn parse_latest_session_notes(log_path: &Path) -> Result<Vec<String>> {
    if !log_path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(log_path)?;

    // Iterate sessions from newest to oldest
    let starts: Vec<usize> = contents
        .match_indices(SESSION_START)
        .map(|(i, _)| i)
        .collect();
    for &start in starts.iter().rev() {
        let session = &contents[start..];
        let notes: Vec<String> = session
            .lines()
            .enumerate()
            .take_while(|(i, l)| *i == 0 || !l.starts_with(SESSION_START))
            .map(|(_, l)| l)
            .filter_map(|line| {
                let after = line.find("note: \"")?.checked_add("note: \"".len())?;
                let rest = line.get(after..)?;
                let end = rest.rfind('"')?;
                Some(rest[..end].to_string())
            })
            .collect();
        if !notes.is_empty() {
            return Ok(notes);
        }
    }
    Ok(Vec::new())
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
#[derive(Debug, Clone, PartialEq)]
pub enum SessionOutcome {
    Success,
    Failed,
    Interrupted,
}

/// Summary of a single session extracted from cryo.log.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_number: u32,
    pub timestamp: NaiveDateTime,
    pub outcome: SessionOutcome,
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

        // Determine outcome — check failure markers before exit code 0,
        // since an agent can exit with code 0 without hibernating (still a failure).
        let outcome = if block.contains("--- CRYO INTERRUPTED ---") {
            SessionOutcome::Interrupted
        } else if block.contains("quick exit detected")
            || block.contains("agent exited without hibernate")
        {
            SessionOutcome::Failed
        } else if block.contains("hibernate:") || block.contains("agent exited (code 0)") {
            SessionOutcome::Success
        } else {
            // Non-zero exit code or unknown outcome — treat as failure
            SessionOutcome::Failed
        };

        summaries.push(SessionSummary {
            session_number,
            timestamp,
            outcome,
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
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_event_logger_session_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        let mut logger = EventLogger::begin(
            &log_path,
            3,
            "Continue parser",
            "claude -p",
            &["feature.md".to_string(), "bug.md".to_string()],
        )
        .unwrap();

        logger.log_event("agent started (pid 12345)").unwrap();
        logger.log_event("note: \"Finished parsing\"").unwrap();
        logger
            .log_event("hibernate: wake=2026-03-09T09:00, exit=0")
            .unwrap();
        logger.finish("agent exited (code 0)").unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("--- CRYO SESSION 3"));
        assert!(content.contains("task: Continue parser"));
        assert!(content.contains("agent: claude -p"));
        assert!(content.contains("inbox: 2 messages (feature.md, bug.md)"));
        assert!(content.contains("note: \"Finished parsing\""));
        assert!(content.contains("--- CRYO END ---"));
    }

    #[test]
    fn test_parse_sessions_since_counts_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        // Session 1: success (agent exited code 0)
        let mut logger = EventLogger::begin(&log_path, 1, "task1", "claude", &[]).unwrap();
        logger.log_event("agent started (pid 100)").unwrap();
        logger
            .log_event("hibernate: wake=2026-03-01T09:00, exit=0")
            .unwrap();
        logger.log_event("agent exited (code 0)").unwrap();
        logger.finish("session complete").unwrap();

        // Session 2: failure (agent exited code 1)
        let mut logger = EventLogger::begin(&log_path, 2, "task2", "claude", &[]).unwrap();
        logger.log_event("agent started (pid 200)").unwrap();
        logger.log_event("agent exited (code 1)").unwrap();
        logger.finish("agent exited without hibernate").unwrap();

        // Session 3: quick exit
        let mut logger = EventLogger::begin(&log_path, 3, "task3", "claude", &[]).unwrap();
        logger.log_event("agent started (pid 300)").unwrap();
        logger
            .log_event("quick exit detected (0.5s without hibernate)")
            .unwrap();
        logger.log_event("agent exited (code 0)").unwrap();
        logger.finish("agent exited without hibernate").unwrap();

        let since =
            chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
                .unwrap();
        let summaries = parse_sessions_since(&log_path, since).unwrap();
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].outcome, SessionOutcome::Success);
        assert_eq!(summaries[1].outcome, SessionOutcome::Failed);
        assert_eq!(summaries[2].outcome, SessionOutcome::Failed);
    }

    #[test]
    fn test_parse_sessions_since_filters_by_time() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        let mut logger = EventLogger::begin(&log_path, 1, "old task", "claude", &[]).unwrap();
        logger.log_event("agent started (pid 100)").unwrap();
        logger.finish("session complete").unwrap();

        // Use a 'since' in the far future to filter out all sessions
        let since =
            chrono::NaiveDateTime::parse_from_str("2099-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
                .unwrap();
        let summaries = parse_sessions_since(&log_path, since).unwrap();
        assert_eq!(summaries.len(), 0);
    }

    #[test]
    fn test_parse_sessions_since_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        {
            let mut logger = EventLogger::begin(&log_path, 1, "task", "claude", &[]).unwrap();
            logger.log_event("agent started (pid 100)").unwrap();
            // Drop without finish → CRYO INTERRUPTED
        }

        let since =
            chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
                .unwrap();
        let summaries = parse_sessions_since(&log_path, since).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].outcome, SessionOutcome::Interrupted);
    }

    #[test]
    fn test_parse_sessions_exit_code_0_without_hibernate_is_failure() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        // Agent exits with code 0 but without hibernating — daemon treats as failure
        let mut logger = EventLogger::begin(&log_path, 1, "task", "claude", &[]).unwrap();
        logger.log_event("agent started (pid 100)").unwrap();
        logger.log_event("agent exited (code 0)").unwrap();
        logger.finish("agent exited without hibernate").unwrap();

        let since =
            chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
                .unwrap();
        let summaries = parse_sessions_since(&log_path, since).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].outcome, SessionOutcome::Failed);
    }

    #[test]
    fn test_parse_sessions_since_empty_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        let since =
            chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
                .unwrap();
        let summaries = parse_sessions_since(&log_path, since).unwrap();
        assert_eq!(summaries.len(), 0);
    }

    #[test]
    fn test_event_logger_drop_writes_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        {
            let mut logger = EventLogger::begin(&log_path, 1, "test", "agent", &[]).unwrap();
            logger.log_event("started").unwrap();
            // Drop without finish
        }

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("CRYO INTERRUPTED"));
    }

    #[test]
    fn test_read_current_session_incomplete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // Session without a CRYO END marker — still in progress
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] agent started\n\
                       [12:00:05] note: \"doing work\"\n";
        std::fs::write(&path, content).unwrap();
        let result = read_current_session(&path).unwrap();
        assert!(
            result.is_some(),
            "Should return content for incomplete session"
        );
        let session = result.unwrap();
        assert!(session.contains("agent started"));
        assert!(session.contains("doing work"));
    }

    #[test]
    fn test_read_latest_session_end_before_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // Only an END marker with no preceding START — should return None
        let content = "--- CRYO END ---\nsome orphaned content\n";
        std::fs::write(&path, content).unwrap();
        let result = read_latest_session(&path).unwrap();
        assert!(result.is_none(), "END before START should return None");
    }

    #[test]
    fn test_read_latest_session_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] first session\n\
                       --- CRYO END ---\n\
                       --- CRYO SESSION 2 | 2026-03-01T13:00:00Z ---\n\
                       [13:00:01] second session\n\
                       --- CRYO END ---\n\
                       --- CRYO SESSION 3 | 2026-03-01T14:00:00Z ---\n\
                       [14:00:01] third session\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let result = read_latest_session(&path).unwrap().unwrap();
        assert!(
            result.contains("third session"),
            "Should return only last session"
        );
        assert!(!result.contains("first session"));
        assert!(!result.contains("second session"));
    }

    #[test]
    fn test_parse_notes_empty_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // Session with no note: lines — should return empty vec
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] agent started\n\
                       [12:00:02] hibernate: wake=2026-03-02T09:00, exit=0\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let notes = parse_latest_session_notes(&path).unwrap();
        assert!(
            notes.is_empty(),
            "Session with no notes should return empty vec"
        );
    }

    #[test]
    fn test_parse_notes_with_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] note: \"simple note\"\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let notes = parse_latest_session_notes(&path).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0], "simple note");
    }

    #[test]
    fn test_parse_notes_truncated_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // note: "unclosed — missing closing quote
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] note: \"unclosed\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let notes = parse_latest_session_notes(&path).unwrap();
        // The parser uses rfind('"') which won't find a closing quote
        // after the content (only the opening quote), so rfind returns
        // index 0 and the slice [..0] is empty — the note is skipped.
        assert!(
            notes.is_empty(),
            "Truncated note with no closing quote should be skipped"
        );
    }

    #[test]
    fn test_parse_wake_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // Actual format: hibernate: wake=TIME, exit=0
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] hibernate: wake=2026-03-01T14:00, exit=0\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let wake = parse_latest_session_wake(&path).unwrap();
        assert_eq!(wake, Some("2026-03-01T14:00".to_string()));
    }

    #[test]
    fn test_parse_wake_no_wake_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // Session with no hibernate line at all
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] plan complete\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let wake = parse_latest_session_wake(&path).unwrap();
        assert!(wake.is_none(), "No wake line should return None");
    }

    #[test]
    fn test_parse_wake_wrong_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // Uses "hibernate, wake_time:" instead of "hibernate: wake=" — parser won't match
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] hibernate, wake_time: 2026-03-01T14:00\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let wake = parse_latest_session_wake(&path).unwrap();
        // The parser looks for "hibernate: wake=" — this format doesn't match
        assert!(
            wake.is_none(),
            "Wrong format should not be recognized as a wake line"
        );
    }

    #[test]
    fn test_parse_task_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        // parse_latest_session_task uses strip_prefix("task: ") on each line,
        // so the task line must start with "task: " (as EventLogger.begin writes it)
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       task: implement auth\n\
                       [12:00:02] agent started\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let task = parse_latest_session_task(&path).unwrap();
        assert!(task.is_some(), "Should find task line");
        assert_eq!(task.unwrap(), "implement auth");
    }

    #[test]
    fn test_parse_task_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                       [12:00:01] agent started\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();
        let task = parse_latest_session_task(&path).unwrap();
        assert!(task.is_none(), "No task line should return None");
    }

    #[test]
    fn test_parse_session_header_valid() {
        let result = parse_session_header("--- CRYO SESSION 5 | 2026-03-01T14:30:45Z ---");
        assert!(result.is_some());
        let (num, ts) = result.unwrap();
        assert_eq!(num, 5);
        assert_eq!(ts.hour(), 14);
        assert_eq!(ts.minute(), 30);
    }

    #[test]
    fn test_parse_session_header_malformed() {
        // Non-numeric session number
        assert!(parse_session_header("--- CRYO SESSION abc | 2026-03-01T14:30:45Z ---").is_none());
        // Invalid timestamp
        assert!(parse_session_header("--- CRYO SESSION 5 | not-a-date ---").is_none());
        // Completely unrelated text
        assert!(parse_session_header("random text").is_none());
    }

    #[test]
    fn test_parse_sessions_since_filters_by_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cryo.log");
        let content = "--- CRYO SESSION 1 | 2026-02-27T10:00:00Z ---\n\
                       [10:00:01] plan complete\n\
                       --- CRYO END ---\n\
                       --- CRYO SESSION 2 | 2026-02-28T10:00:00Z ---\n\
                       [10:00:01] plan complete\n\
                       --- CRYO END ---\n\
                       --- CRYO SESSION 3 | 2026-03-01T10:00:00Z ---\n\
                       [10:00:01] agent exited without hibernate\n\
                       --- CRYO END ---\n";
        std::fs::write(&path, content).unwrap();

        let since = chrono::NaiveDate::from_ymd_opt(2026, 2, 28)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let sessions = parse_sessions_since(&path, since).unwrap();
        assert_eq!(sessions.len(), 2, "Should return sessions 2 and 3");
        assert_eq!(sessions[0].session_number, 2);
        assert_eq!(sessions[1].session_number, 3);
    }
}
