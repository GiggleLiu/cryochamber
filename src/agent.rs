// src/agent.rs
use anyhow::{Context, Result};
use chrono::Local;
use std::process::Command;

use crate::message::Message;

/// Supported agent types.
enum AgentKind {
    /// Claude Code: `claude [flags] -p <prompt>`
    Claude,
    /// OpenCode: `opencode run [flags] <prompt>`
    Opencode,
    /// Codex: `codex exec [flags] <prompt>`
    Codex,
    /// Custom agent: `<program> [args] <prompt>` (prompt as positional arg)
    Custom,
}

/// Resolve a user-provided agent name to a supported agent kind and full command.
///
/// Known agents get automatic subcommand injection:
///   "opencode"  → "opencode run"   (bare opencode starts interactive TUI)
///   "codex"     → "codex exec"     (bare codex starts interactive TUI)
///   "claude"    → "claude -p"      (needs -p flag for non-interactive mode)
///
/// Unknown programs are treated as custom agents (prompt passed as positional arg).
fn resolve_agent(agent_cmd: &str) -> Result<(AgentKind, String, Vec<String>)> {
    let parts = shell_words::split(agent_cmd.trim()).context("Failed to parse agent command")?;
    let program = parts.first().context("Agent command is empty")?;
    let exe = program.rsplit('/').next().unwrap_or(program);
    let args = parts[1..].to_vec();

    match exe {
        "claude" => Ok((AgentKind::Claude, program.clone(), args)),
        "opencode" => {
            let mut full_args = args;
            if full_args.first().map(|s| s.as_str()) != Some("run") {
                full_args.insert(0, "run".to_string());
            }
            Ok((AgentKind::Opencode, program.clone(), full_args))
        }
        "codex" => {
            let mut full_args = args;
            if full_args.first().map(|s| s.as_str()) != Some("exec") {
                full_args.insert(0, "exec".to_string());
            }
            // codex exec requires --full-auto for non-interactive use
            if !full_args.iter().any(|a| a == "--full-auto") {
                full_args.push("--full-auto".to_string());
            }
            // Skip git repo check — cryo projects aren't always inside git repos
            if !full_args.iter().any(|a| a == "--skip-git-repo-check") {
                full_args.push("--skip-git-repo-check".to_string());
            }
            Ok((AgentKind::Codex, program.clone(), full_args))
        }
        _ => Ok((AgentKind::Custom, program.clone(), args)),
    }
}

/// Return the executable name for preflight validation.
pub fn agent_program(agent_cmd: &str) -> Result<String> {
    let (_, program, _) = resolve_agent(agent_cmd)?;
    Ok(program)
}

pub struct AgentConfig {
    pub log_content: Option<String>,
    pub session_number: u32,
    pub task: String,
    pub inbox_messages: Vec<Message>,
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

- Use `cryo hibernate` to end your session (--wake or --complete)
- Use `cryo note` to leave context for your next session
- Check `cryo inbox` for messages from the human
- Read plan.md before starting work
"#,
        session_number = config.session_number,
        task = config.task,
        history = history_section,
        messages = messages_section,
    )
}

/// Build a `Command` for the given agent, ready to execute with the prompt.
pub fn build_command(agent_command: &str, prompt: &str) -> Result<Command> {
    let (kind, program, args) = resolve_agent(agent_command)?;

    let mut cmd = Command::new(&program);
    cmd.args(&args);

    match kind {
        AgentKind::Claude => {
            cmd.arg("-p");
        }
        AgentKind::Opencode | AgentKind::Codex | AgentKind::Custom => {}
    }
    cmd.arg(prompt);

    Ok(cmd)
}

/// Spawn agent as a child process. Does NOT capture stdout/stderr.
/// Returns the Child handle for the daemon to monitor.
pub fn spawn_agent(agent_command: &str, prompt: &str) -> anyhow::Result<std::process::Child> {
    let mut cmd = build_command(agent_command, prompt)?;
    let child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn agent: {e}"))?;
    Ok(child)
}
