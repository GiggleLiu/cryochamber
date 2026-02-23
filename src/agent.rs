// src/agent.rs
use anyhow::{Context, Result};
use chrono::Local;
use std::process::Command;

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
    let parts = shell_words::split(agent_command).context("Failed to parse agent command")?;
    let (program, args) = parts.split_first().context("Agent command is empty")?;

    let output = Command::new(program)
        .args(args)
        .arg("--prompt")
        .arg(prompt)
        .output()
        .context(format!("Failed to spawn agent: {agent_command}"))?;

    Ok(AgentResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
