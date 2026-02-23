// src/log.rs
use anyhow::Result;
use chrono::Local;
use std::fs;
use std::io::Write;
use std::path::Path;

const SESSION_START: &str = "--- CRYO SESSION";
const SESSION_END: &str = "--- CRYO END ---";

pub struct Session {
    pub number: u32,
    pub task: String,
    pub output: String,
    pub stderr: Option<String>,
}

pub fn append_session(log_path: &Path, session: &Session) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S");
    writeln!(file, "{SESSION_START} {timestamp} ---")?;
    writeln!(file, "Session: {}", session.number)?;
    writeln!(file, "Task: {}", session.task)?;
    writeln!(file)?;
    writeln!(file, "{}", session.output)?;
    if let Some(stderr) = &session.stderr {
        if !stderr.trim().is_empty() {
            writeln!(file)?;
            writeln!(file, "--- STDERR ---")?;
            writeln!(file, "{stderr}")?;
        }
    }
    writeln!(file, "{SESSION_END}")?;
    writeln!(file)?;

    Ok(())
}

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
