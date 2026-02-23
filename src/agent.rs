// src/agent.rs
use anyhow::{Context, Result};
use chrono::Local;
use std::io::BufRead;
use std::process::{Command, Stdio};

use crate::log::SessionWriter;
use crate::message::Message;

pub struct AgentConfig {
    pub log_content: Option<String>,
    pub session_number: u32,
    pub task: String,
    pub inbox_messages: Vec<Message>,
}

pub struct AgentResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn build_prompt(config: &AgentConfig) -> String {
    let current_time = Local::now().format("%Y-%m-%dT%H:%M:%S");

    let history_section = match &config.log_content {
        Some(log) => format!("\n## Previous Session Log\n\n{log}\n"),
        None => "\n## Previous Session Log\n\nNo previous sessions.\n".to_string(),
    };

    let messages_section = if config.inbox_messages.is_empty() {
        String::new()
    } else {
        let count = config.inbox_messages.len();
        let mut text = format!("\n## New Messages ({count} unread)\n\n");
        for msg in &config.inbox_messages {
            let ts = msg.timestamp.format("%Y-%m-%dT%H:%M");
            text.push_str(&format!("### From: {} ({})\n", msg.from, ts));
            if !msg.subject.is_empty() {
                text.push_str(&format!("Subject: {}\n", msg.subject));
            }
            text.push_str(&format!("\n{}\n\n---\n\n", msg.body));
        }
        text
    };

    format!(
        r#"# Cryochamber Session

Current time: {current_time}
Session number: {session_number}

## Instructions

Follow the cryochamber protocol in CLAUDE.md or AGENTS.md. Read plan.md for the full plan.

## Your Task

{task}
{history}{messages}
## Reminders

- Write markers at the end of your response:
  - `[CRYO:EXIT 0] summary` (0=success, 1=partial, 2=failure)
  - `[CRYO:WAKE 2026-03-08T09:00]` (omit only if plan is complete)
  - `[CRYO:PLAN note]` to leave notes for your future self
- Read plan.md before starting work
"#,
        session_number = config.session_number,
        task = config.task,
        history = history_section,
        messages = messages_section,
    )
}

pub fn run_agent(agent_command: &str, prompt: &str) -> Result<AgentResult> {
    run_agent_streaming(agent_command, prompt, None)
}

/// Run the agent, streaming stdout lines to a `SessionWriter` in real-time.
/// If `writer` is None, output is only captured (no streaming to log).
pub fn run_agent_streaming(
    agent_command: &str,
    prompt: &str,
    mut writer: Option<&mut SessionWriter>,
) -> Result<AgentResult> {
    let parts = shell_words::split(agent_command).context("Failed to parse agent command")?;
    let (program, args) = parts.split_first().context("Agent command is empty")?;

    let mut child = Command::new(program)
        .args(args)
        .arg("--prompt")
        .arg(prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!("Failed to spawn agent: {agent_command}"))?;

    let child_stdout = child.stdout.take().unwrap();
    let child_stderr = child.stderr.take().unwrap();

    // Read stderr in a background thread so it doesn't block stdout reading.
    let stderr_handle = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(child_stderr);
        let mut buf = String::new();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    buf.push_str(&l);
                    buf.push('\n');
                }
                Err(_) => break,
            }
        }
        buf
    });

    // Stream stdout line-by-line, writing to log in real-time.
    let mut stdout_buf = String::new();
    let reader = std::io::BufReader::new(child_stdout);
    for line in reader.lines() {
        match line {
            Ok(l) => {
                if let Some(ref mut w) = writer {
                    let _ = w.write_line(&l);
                }
                stdout_buf.push_str(&l);
                stdout_buf.push('\n');
            }
            Err(_) => break,
        }
    }

    let status = child.wait().context("Failed to wait for agent process")?;
    let stderr_buf = stderr_handle.join().unwrap_or_default();

    Ok(AgentResult {
        stdout: stdout_buf,
        stderr: stderr_buf,
        exit_code: status.code().unwrap_or(-1),
    })
}
