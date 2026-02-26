// src/log.rs
use anyhow::Result;
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

pub fn session_count(log_path: &Path) -> Result<u32> {
    if !log_path.exists() {
        return Ok(0);
    }
    let contents = fs::read_to_string(log_path)?;
    Ok(contents.matches(SESSION_START).count() as u32)
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
}
