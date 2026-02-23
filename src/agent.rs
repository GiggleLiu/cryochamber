// src/agent.rs
use anyhow::{Context, Result};
use chrono::Local;
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::log::SessionWriter;
use crate::message::Message;

/// Send a signal to a process, logging a warning on failure.
fn send_signal(pid: u32, signal: i32) {
    let ret = unsafe { libc::kill(pid as i32, signal) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        eprintln!("Warning: failed to send signal {signal} to PID {pid}: {err}");
    }
}

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
    let mut cmd = build_command(agent_command, prompt)?;

    let mut child = cmd
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
                    if let Err(e) = w.write_line(&l) {
                        eprintln!("Warning: failed to write to session log: {e}");
                    }
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

/// Run the agent with a timeout watchdog. Sends SIGTERM then SIGKILL if exceeded.
///
/// `shutdown` is an optional external signal (e.g. from daemon SIGINT handler).
/// When set, the agent is terminated early.
pub fn run_agent_with_timeout(
    agent_command: &str,
    prompt: &str,
    writer: &mut SessionWriter,
    timeout_secs: u64,
    shutdown: Option<Arc<AtomicBool>>,
) -> Result<AgentResult> {
    let mut child = build_command(agent_command, prompt)?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!("Failed to spawn agent: {agent_command}"))?;

    let child_pid = child.id();
    let child_done = Arc::new(AtomicBool::new(false));
    let child_done_clone = Arc::clone(&child_done);

    // Spawn watchdog thread: enforces timeout and/or external shutdown signal.
    // timeout_secs == 0 means no timeout, but shutdown is still monitored.
    let has_timeout = timeout_secs > 0;
    let timeout_handle = std::thread::spawn(move || {
        let deadline = has_timeout
            .then(|| std::time::Instant::now() + Duration::from_secs(timeout_secs));
        loop {
            if child_done_clone.load(Ordering::Relaxed) {
                return false; // child exited normally
            }
            if let Some(d) = deadline {
                if std::time::Instant::now() >= d {
                    eprintln!("Session timeout ({timeout_secs}s) — killing agent");
                    send_signal(child_pid, libc::SIGTERM);
                    std::thread::sleep(Duration::from_secs(5));
                    send_signal(child_pid, libc::SIGKILL);
                    return true; // timed out
                }
            }
            if let Some(ref s) = shutdown {
                if s.load(Ordering::Relaxed) {
                    send_signal(child_pid, libc::SIGTERM);
                    std::thread::sleep(Duration::from_secs(5));
                    if !child_done_clone.load(Ordering::Relaxed) {
                        send_signal(child_pid, libc::SIGKILL);
                    }
                    return false;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });

    let child_stdout = child.stdout.take().unwrap();
    let child_stderr = child.stderr.take().unwrap();

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

    let mut stdout_buf = String::new();
    let reader = std::io::BufReader::new(child_stdout);
    for line in reader.lines() {
        match line {
            Ok(l) => {
                if let Err(e) = writer.write_line(&l) {
                    eprintln!("Warning: failed to write to session log: {e}");
                }
                stdout_buf.push_str(&l);
                stdout_buf.push('\n');
            }
            Err(_) => break,
        }
    }

    let status = child.wait().context("Failed to wait for agent process")?;
    child_done.store(true, Ordering::Relaxed);
    let stderr_buf = stderr_handle.join().unwrap_or_default();
    let timed_out = timeout_handle.join().unwrap_or(false);

    if timed_out {
        anyhow::bail!("Agent killed after {timeout_secs}s timeout");
    }

    Ok(AgentResult {
        stdout: stdout_buf,
        stderr: stderr_buf,
        exit_code: status.code().unwrap_or(-1),
    })
}
