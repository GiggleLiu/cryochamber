// src/agent.rs
use anyhow::{Context, Result};
use chrono::Local;
use std::process::Command;

pub struct AgentConfig {
    pub plan_content: String,
    pub log_content: Option<String>,
    pub session_number: u32,
    pub task: String,
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

    format!(
        r#"# Cryochamber Protocol

You are running inside cryochamber, a long-term task scheduler.
You will be hibernated after this session and woken up later.

## Your Context

Current time: {current_time}
Session number: {session_number}

## Your Plan

{plan}
{history}
## Your Task

{task}

## After Completing Your Task

You MUST write the following markers at the end of your response.

### Required:
[CRYO:EXIT <code>] <one-line summary>
  - 0 = success
  - 1 = partial success
  - 2 = failure

### Optional (write these if the plan has more work):
[CRYO:WAKE <ISO8601 datetime>]       — when to wake up next
[CRYO:CMD <command to run on wake>]   — what to execute (default: re-run same command)
[CRYO:PLAN <note for future self>]    — context you want to remember next session
[CRYO:FALLBACK <action> <target> "<message>"]  — dead man's switch
  - action: email, webhook
  - example: [CRYO:FALLBACK email user@example.com "weekly review did not run"]

### Rules:
- No WAKE marker = plan is complete, no more wake-ups
- Always read the plan and previous session log above before starting
- PLAN markers are your memory — use them to leave notes for yourself
"#,
        session_number = config.session_number,
        plan = config.plan_content,
        history = history_section,
        task = config.task,
    )
}

pub fn run_agent(agent_command: &str, prompt: &str) -> Result<AgentResult> {
    let parts: Vec<&str> = agent_command.split_whitespace().collect();
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
