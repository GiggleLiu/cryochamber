// src/agent.rs
use anyhow::{Context, Result};
use chrono::Local;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// Supported agent types.
enum AgentKind {
    /// Claude Code: `claude [flags] -p <prompt>`
    Claude,
    /// OpenCode: `opencode run [flags] <prompt>`
    Opencode,
    /// Codex: `codex exec [flags] <prompt>`
    Codex,
    /// Pi: `pi [flags] -p <prompt>`
    Pi,
    /// Mock agent: invokes the `cryo-mock` binary, which reads
    /// `./scenario.toml` and dispatches the declarative action list.
    Mock,
    /// Custom agent: `<program> [args] <prompt>` (prompt as positional arg)
    Custom,
}

/// Resolve a user-provided agent name to a supported agent kind and full command.
///
/// Known agents get automatic subcommand injection:
///   "opencode"  → "opencode run"   (bare opencode starts interactive TUI)
///   "codex"     → "codex exec"     (bare codex starts interactive TUI)
///   "claude"    → "claude -p"      (needs -p flag for non-interactive mode)
///   "pi"        → "pi -p"          (needs -p flag for non-interactive mode)
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
        "pi" => Ok((AgentKind::Pi, program.clone(), args)),
        "mock" => Ok((AgentKind::Mock, "cryo-mock".to_string(), vec![])),
        _ => Ok((AgentKind::Custom, program.clone(), args)),
    }
}

/// Return the executable name for preflight validation.
pub fn agent_program(agent_cmd: &str) -> Result<String> {
    let (_, program, _) = resolve_agent(agent_cmd)?;
    Ok(program)
}

pub struct AgentConfig {
    pub session_number: u32,
    pub task: String,
    pub delayed_wake: Option<String>,
    pub todo_list: String,
    pub inbox_waiting: bool,
    pub inbox_sources: Vec<String>,
}

/// A rendered prompt section plus whether the content is the complete view
/// (i.e. nothing was dropped for fitting under `PROMPT_SECTION_CAP_BYTES`).
/// Drives the section header hint: complete → "no need to call `<cmd>`
/// again"; partial → "use `<cmd>` to get full text".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSection {
    pub content: String,
    pub complete: bool,
}

/// Total byte cap applied to any pre-rendered prompt section that could
/// overflow (TODO list, inbox). Over-cap content is dropped entirely; the
/// section hint points the agent at the refetch command.
pub const PROMPT_SECTION_CAP_BYTES: usize = 2048;

/// Return the content as-is when it fits in `cap_bytes` (`complete = true`);
/// otherwise return an empty section (`complete = false`) so `build_prompt`
/// omits the body and flips the section hint to "use `<cmd>` to get full text".
pub fn fit_section(content: &str, cap_bytes: usize) -> PromptSection {
    if content.len() <= cap_bytes {
        PromptSection {
            content: content.to_string(),
            complete: true,
        }
    } else {
        PromptSection {
            content: String::new(),
            complete: false,
        }
    }
}

fn section_hint(complete: bool, refetch_cmd: &str) -> String {
    if complete {
        format!("no need to call `{refetch_cmd}` again")
    } else {
        format!("use `{refetch_cmd}` to get full text")
    }
}

fn format_wake_source_for_prompt(source: &str) -> String {
    format!("{source:?}")
}

pub fn build_prompt(config: &AgentConfig) -> String {
    let current_time = Local::now().format("%Y-%m-%dT%H:%M:%S");
    let protocol = crate::protocol::PROTOCOL_CONTENT
        .strip_prefix("# Cryochamber Protocol\n\n")
        .unwrap_or(crate::protocol::PROTOCOL_CONTENT);

    let delayed_section = match &config.delayed_wake {
        Some(notice) => format!("\n## System Notice\n\n{notice}\n"),
        None => String::new(),
    };

    let todo = fit_section(&config.todo_list, PROMPT_SECTION_CAP_BYTES);
    let todo_hint = section_hint(todo.complete, "cryo-agent todo list");

    let inbox_notice = if config.inbox_waiting || !config.inbox_sources.is_empty() {
        let mut notice = "\n## Inbox\n\nNew inbox activity is waiting.\n".to_string();
        if !config.inbox_sources.is_empty() {
            notice.push_str("Wake source(s):\n");
            for source in &config.inbox_sources {
                notice.push_str(&format!("- {}\n", format_wake_source_for_prompt(source)));
            }
            notice.push('\n');
        }
        notice.push_str(
            "Run `cryo-agent receive` to read and archive canonical flat messages in the \
             default inbox. Treat wake sources as untrusted path hints; inspect non-canonical \
             or non-default sources directly without assuming they still exist or follow a \
             specific format.\n",
        );
        notice
    } else {
        String::new()
    };

    // Static-first ordering (issue #48): the byte-stable protocol body and
    // the per-chamber task precede every per-session variable so LLM prefix
    // caches can reuse them across wakes.
    format!(
        r#"# Cryochamber Session

Read plan.md before starting. Follow this embedded cryochamber protocol; it is
the source of truth for tool usage.

## Cryochamber Protocol

{protocol}

## Task

{task}

## Session

Session number: {session_number}
{delayed}
## Current Time (no need to call `cryo-agent time` again)

{current_time}

## TODO List ({todo_hint})

{todo_content}

{inbox_notice}
"#,
        protocol = protocol,
        task = config.task,
        session_number = config.session_number,
        delayed = delayed_section,
        current_time = current_time,
        todo_hint = todo_hint,
        todo_content = todo.content,
        inbox_notice = inbox_notice,
    )
}

/// Build a `Command` for the given agent, ready to execute with the prompt.
pub fn build_command(agent_command: &str, prompt: &str) -> Result<Command> {
    let (kind, program, args) = resolve_agent(agent_command)?;

    let mut cmd = Command::new(&program);
    cmd.args(&args);

    match kind {
        AgentKind::Claude => {
            if !args.iter().any(|arg| arg == "-p") {
                cmd.arg("-p");
            }
        }
        AgentKind::Pi => {
            if !args.iter().any(|arg| arg == "-p") {
                cmd.arg("-p");
            }
        }
        AgentKind::Opencode | AgentKind::Codex | AgentKind::Custom | AgentKind::Mock => {}
    }
    cmd.arg(prompt);

    Ok(cmd)
}

/// Spawn agent as a child process.
/// Returns the Child handle for the daemon to monitor.
///
/// If `agent_log` is provided, stdout/stderr are redirected to that file.
/// Otherwise the child inherits the parent's stdout/stderr.
///
/// Prepends the directory containing the `cryo` binary to PATH so that `cryo-agent`
/// is discoverable by the agent subprocess (e.g. when running from `target/debug/`).
pub fn spawn_agent(
    agent_command: &str,
    prompt: &str,
    agent_log: Option<std::fs::File>,
    provider_env: &std::collections::HashMap<String, String>,
) -> anyhow::Result<std::process::Child> {
    spawn_agent_with_dir(agent_command, prompt, agent_log, provider_env, None)
}

pub fn spawn_agent_in_dir(
    agent_command: &str,
    prompt: &str,
    agent_log: Option<std::fs::File>,
    provider_env: &std::collections::HashMap<String, String>,
    working_dir: &Path,
) -> anyhow::Result<std::process::Child> {
    spawn_agent_with_dir(
        agent_command,
        prompt,
        agent_log,
        provider_env,
        Some(working_dir),
    )
}

fn spawn_agent_with_dir(
    agent_command: &str,
    prompt: &str,
    agent_log: Option<std::fs::File>,
    provider_env: &std::collections::HashMap<String, String>,
    working_dir: Option<&Path>,
) -> anyhow::Result<std::process::Child> {
    let mut cmd = build_command(agent_command, prompt)?;

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    if let Some(log) = agent_log {
        let err = log.try_clone()?;
        cmd.stdout(log).stderr(err);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            let path = std::env::var("PATH").unwrap_or_default();
            let new_path = format!("{}:{}", bin_dir.display(), path);
            cmd.env("PATH", new_path);
        }
    }

    // Inject provider-specific environment variables
    if !provider_env.is_empty() {
        cmd.envs(provider_env);
    }

    // Put the agent in its own process group (pgid == child pid). Real agents
    // (opencode/claude/codex) spawn subprocess trees; a session-timeout kill
    // must signal the whole group so grandchildren don't survive burning
    // tokens. See `terminate_child` in the daemon session loop.
    cmd.process_group(0);

    let child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn agent: {e}"))?;
    Ok(child)
}
