// src/main.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use cryochamber::agent::{self, AgentConfig};
use cryochamber::log::{self, Session};
use cryochamber::marker;
use cryochamber::message;
use cryochamber::protocol;
use cryochamber::state::{self, CryoState};
use cryochamber::timer;
use cryochamber::validate;

#[derive(Parser)]
#[command(name = "cryo", about = "Long-term AI agent task scheduler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a working directory with protocol file and template plan
    Init {
        /// Agent command to target (determines CLAUDE.md vs AGENTS.md)
        #[arg(long, default_value = "opencode")]
        agent: String,
    },
    /// Begin a new plan: initialize and run the first task
    Start {
        /// Path to the natural language plan file
        plan: PathBuf,
        /// Agent command to use (default: opencode)
        #[arg(long, default_value = "opencode")]
        agent: String,
        /// Max retry attempts on agent spawn failure (default: 1 = no retry)
        #[arg(long, default_value = "1")]
        max_retries: u32,
    },
    /// Called by OS timer: execute the next scheduled task
    Wake {
        /// Force wake immediately (cancel existing timer)
        #[arg(long)]
        now: bool,
    },
    /// Show current status: next wake time, last result
    Status,
    /// Cancel all timers and stop the schedule
    Cancel,
    /// Run pre-hibernate validation checks
    Validate,
    /// Print the session log
    Log,
    /// Show current time, or compute a future time from an offset
    ///
    /// Examples:
    ///   cryo time              # prints current time
    ///   cryo time "+1 day"     # 1 day from now
    ///   cryo time "+2 hours"   # 2 hours from now
    ///   cryo time "+30 minutes" # 30 minutes from now
    Time {
        /// Optional offset: "+N unit" where unit is minutes/hours/days/weeks
        offset: Option<String>,
    },
    /// Execute a fallback action (used internally by timers)
    FallbackExec {
        action: String,
        target: String,
        message: String,
    },
    /// GitHub Discussion sync utility (independent message sync service)
    Gh {
        #[command(subcommand)]
        action: GhCommands,
    },
}

#[derive(Subcommand)]
enum GhCommands {
    /// Initialize: create a Discussion and write gh-sync.json
    Init {
        /// GitHub repo in "owner/repo" format
        #[arg(long)]
        repo: String,
        /// Discussion title (default: derived from plan.md)
        #[arg(long)]
        title: Option<String>,
    },
    /// Pull new Discussion comments into messages/inbox/
    Pull,
    /// Push session summary + CRYO:REPLY markers to Discussion
    Push,
    /// Pull then push (full sync)
    Sync,
    /// Show sync status
    Status,
}

fn work_dir() -> Result<PathBuf> {
    std::env::current_dir().context("Failed to get current directory")
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("timer.json")
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join("cryo.log")
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { agent } => cmd_init(&agent),
        Commands::Start {
            plan,
            agent,
            max_retries,
        } => cmd_start(&plan, &agent, max_retries),
        Commands::Time { offset } => cmd_time(offset.as_deref()),
        Commands::Wake { now } => cmd_wake(now),
        Commands::Status => cmd_status(),
        Commands::Cancel => cmd_cancel(),
        Commands::Validate => cmd_validate(),
        Commands::Log => cmd_log(),
        Commands::FallbackExec {
            action,
            target,
            message,
        } => {
            let dir = work_dir()?;
            let fb = cryochamber::fallback::FallbackAction {
                action,
                target,
                message,
            };
            fb.execute(&dir)
        }
        Commands::Gh { action } => match action {
            GhCommands::Init { repo, title } => cmd_gh_init(&repo, title.as_deref()),
            GhCommands::Pull => cmd_gh_pull(),
            GhCommands::Push => cmd_gh_push(),
            GhCommands::Sync => {
                cmd_gh_pull()?;
                cmd_gh_push()
            }
            GhCommands::Status => cmd_gh_status(),
        },
    }
}

fn cmd_init(agent_cmd: &str) -> Result<()> {
    let dir = work_dir()?;

    let filename = protocol::protocol_filename(agent_cmd);
    if protocol::write_protocol_file(&dir, filename)? {
        println!("Wrote {filename} (cryochamber protocol)");
    } else {
        println!("{filename} already exists, skipping");
    }

    if protocol::write_template_plan(&dir)? {
        println!("Wrote template plan.md");
    } else {
        println!("plan.md already exists, skipping");
    }

    message::ensure_dirs(&dir)?;
    println!("Created messages/ directory");

    println!("\nCryochamber initialized. Next steps:");
    println!("  1. Edit plan.md with your task plan");
    println!("  2. Run: cryo start plan.md");

    Ok(())
}

fn cmd_start(plan_path: &Path, agent_cmd: &str, max_retries: u32) -> Result<()> {
    let dir = work_dir()?;

    // Auto-init: ensure the agent-specific protocol file and message dirs exist
    let filename = protocol::protocol_filename(agent_cmd);
    if protocol::write_protocol_file(&dir, filename)? {
        println!("Wrote {filename} (cryochamber protocol)");
    }
    message::ensure_dirs(&dir)?;

    let plan_dest = dir.join("plan.md");

    let should_copy = match (
        std::fs::canonicalize(plan_path),
        std::fs::canonicalize(&plan_dest),
    ) {
        (Ok(src), Ok(dst)) => src != dst,
        _ => true,
    };
    if should_copy {
        std::fs::copy(plan_path, &plan_dest).context("Failed to copy plan file")?;
    }

    let mut cryo_state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: Some(agent_cmd.to_string()),
        wake_timer_id: None,
        fallback_timer_id: None,
        pid: Some(std::process::id()),
        max_retries,
        retry_count: 0,
    };
    state::save_state(&state_path(&dir), &cryo_state)?;

    println!("Cryochamber initialized. Running first task...");

    run_session(
        &dir,
        &mut cryo_state,
        agent_cmd,
        "Execute the first task from the plan",
    )?;

    Ok(())
}

fn cmd_wake(force: bool) -> Result<()> {
    let dir = work_dir()?;
    let mut cryo_state = state::load_state(&state_path(&dir))?
        .context("No cryochamber state found. Run 'cryo start' first.")?;

    if force {
        if let Some(wake_id) = &cryo_state.wake_timer_id {
            let timer_impl = timer::create_timer()?;
            match timer_impl.cancel(&timer::TimerId(wake_id.clone())) {
                Ok(()) => {
                    cryo_state.wake_timer_id = None;
                    println!("Cancelled existing wake timer. Running session now.");
                }
                Err(e) => {
                    eprintln!("Warning: failed to cancel wake timer: {e}");
                    eprintln!("The old timer may still fire. Proceeding anyway.");
                    cryo_state.wake_timer_id = None;
                }
            }
        }
    }

    if state::is_locked(&cryo_state) && cryo_state.pid != Some(std::process::id()) {
        anyhow::bail!(
            "Another cryochamber session is running (PID: {:?})",
            cryo_state.pid
        );
    }

    // Validate workspace: plan.md must exist
    let plan_path = dir.join("plan.md");
    if !plan_path.exists() {
        anyhow::bail!(
            "plan.md not found in {}. Cannot wake without a plan file.",
            dir.display()
        );
    }

    cryo_state.pid = Some(std::process::id());
    cryo_state.session_number += 1;
    state::save_state(&state_path(&dir), &cryo_state)?;

    let agent_cmd = cryo_state
        .last_command
        .clone()
        .unwrap_or_else(|| "opencode".to_string());

    let task = get_task_from_log(&dir).unwrap_or_else(|| "Continue the plan".to_string());

    if let Some(fb_id) = &cryo_state.fallback_timer_id {
        let timer_impl = timer::create_timer()?;
        let _ = timer_impl.cancel(&timer::TimerId(fb_id.clone()));
        cryo_state.fallback_timer_id = None;
    }

    run_session(&dir, &mut cryo_state, &agent_cmd, &task)?;

    Ok(())
}

fn run_session(dir: &Path, cryo_state: &mut CryoState, agent_cmd: &str, task: &str) -> Result<()> {
    let log = log_path(dir);

    // Inner function so we can guarantee PID cleanup on all exit paths.
    let result = run_session_inner(dir, cryo_state, agent_cmd, task, &log);

    // Always clear PID and save state, even on error.
    cryo_state.pid = None;
    state::save_state(&state_path(dir), cryo_state)?;

    result
}

fn run_session_inner(
    dir: &Path,
    cryo_state: &mut CryoState,
    agent_cmd: &str,
    task: &str,
    log: &Path,
) -> Result<()> {
    let log_content = cryochamber::log::read_latest_session(log)?;
    let inbox = message::read_inbox(dir)?;
    let inbox_messages: Vec<_> = inbox.iter().map(|(_, msg)| msg.clone()).collect();
    let inbox_filenames: Vec<_> = inbox.into_iter().map(|(f, _)| f).collect();

    let config = AgentConfig {
        log_content,
        session_number: cryo_state.session_number,
        task: task.to_string(),
        inbox_messages,
    };
    let prompt = agent::build_prompt(&config);

    println!("Session #{}: Running agent...", cryo_state.session_number);

    // Run agent with retry on spawn failure
    let result = run_agent_with_retry(cryo_state, agent_cmd, &prompt, log, task)?;

    // Agent ran successfully — reset retry counter
    cryo_state.retry_count = 0;

    if result.exit_code != 0 {
        eprintln!(
            "Agent exited with code {}. Stderr:\n{}",
            result.exit_code, result.stderr
        );
    }

    let session = Session {
        number: cryo_state.session_number,
        task: task.to_string(),
        output: result.stdout.clone(),
        stderr: Some(result.stderr.clone()),
    };
    log::append_session(log, &session)?;

    // Archive processed inbox messages
    if !inbox_filenames.is_empty() {
        message::archive_messages(dir, &inbox_filenames)?;
    }

    let markers = marker::parse_markers(&result.stdout)?;

    let validation = validate::validate_markers(&markers);

    for warning in &validation.warnings {
        eprintln!("Warning: {warning}");
    }

    if validation.plan_complete {
        println!("Plan complete! No more wake-ups scheduled.");
        return Ok(());
    }

    for error in &validation.errors {
        eprintln!("Error: {error}");
    }

    if !validation.can_hibernate {
        anyhow::bail!("Pre-hibernate validation failed. Not hibernating.");
    }

    // Schedule next wake
    let timer_impl = timer::create_timer()?;
    // wake_time is Option<WakeTime> — extract the inner NaiveDateTime
    let wake_time = markers.wake_time.unwrap().0;
    let dir_str = dir.to_string_lossy().to_string();

    let wake_cmd = format!("{} wake", std::env::current_exe()?.to_string_lossy());

    let wake_id = timer_impl.schedule_wake(wake_time, &wake_cmd, &dir_str)?;
    cryo_state.wake_timer_id = Some(wake_id.0.clone());

    if let Some(cmd) = &markers.command {
        cryo_state.last_command = Some(cmd.clone());
    }

    if let Some(fb) = markers.fallbacks.first() {
        let fallback_time = wake_time + chrono::Duration::hours(1);
        let fb_id = timer_impl.schedule_fallback(fallback_time, fb, &dir_str)?;
        cryo_state.fallback_timer_id = Some(fb_id.0.clone());
    }

    // Verify timer
    let status = timer_impl.verify(&timer::TimerId(cryo_state.wake_timer_id.clone().unwrap()))?;
    match status {
        timer::TimerStatus::Scheduled { .. } => {
            println!(
                "Hibernating. Next wake: {}",
                wake_time.format("%Y-%m-%d %H:%M")
            );
        }
        timer::TimerStatus::NotFound => {
            anyhow::bail!("Timer registration verification failed!");
        }
    }

    Ok(())
}

/// Attempt to run the agent, retrying on spawn failure up to `max_retries` times.
/// Each failed attempt is logged. Returns the successful `AgentResult` or bails after exhausting retries.
fn run_agent_with_retry(
    cryo_state: &mut CryoState,
    agent_cmd: &str,
    prompt: &str,
    log: &Path,
    task: &str,
) -> Result<agent::AgentResult> {
    let max_attempts = cryo_state.max_retries;
    let mut last_err = String::new();

    for attempt in 1..=max_attempts {
        match agent::run_agent(agent_cmd, prompt) {
            Ok(r) => return Ok(r),
            Err(e) => {
                cryo_state.retry_count = attempt;
                last_err = format!("Agent failed to run (attempt {attempt}/{max_attempts}): {e}");
                eprintln!("{last_err}");

                let session = Session {
                    number: cryo_state.session_number,
                    task: task.to_string(),
                    output: last_err.clone(),
                    stderr: None,
                };
                log::append_session(log, &session)?;

                if attempt < max_attempts {
                    let delay = std::time::Duration::from_secs(5 * u64::from(attempt));
                    eprintln!("Retrying in {}s...", delay.as_secs());
                    std::thread::sleep(delay);
                }
            }
        }
    }

    anyhow::bail!("{last_err}")
}

fn get_task_from_log(dir: &Path) -> Option<String> {
    let log = log_path(dir);
    let latest = cryochamber::log::read_latest_session(&log).ok()??;
    let markers = marker::parse_markers(&latest).ok()?;
    markers.command.or(markers.plan_note)
}

fn cmd_status() -> Result<()> {
    let dir = work_dir()?;
    let cryo_state = state::load_state(&state_path(&dir))?;

    match cryo_state {
        None => println!("No cryochamber instance in this directory."),
        Some(state) => {
            println!("Plan: {}", state.plan_path);
            println!("Session: {}", state.session_number);
            println!(
                "Wake timer: {}",
                state.wake_timer_id.as_deref().unwrap_or("none")
            );
            println!(
                "Fallback timer: {}",
                state.fallback_timer_id.as_deref().unwrap_or("none")
            );
            println!(
                "Locked by PID: {}",
                state.pid.map(|p| p.to_string()).unwrap_or("none".into())
            );

            let log = log_path(&dir);
            if let Some(latest) = cryochamber::log::read_latest_session(&log)? {
                println!("\n--- Latest session ---");
                let lines: Vec<&str> = latest.lines().collect();
                let start = lines.len().saturating_sub(10);
                for line in &lines[start..] {
                    println!("{line}");
                }
            }
        }
    }
    Ok(())
}

fn cmd_cancel() -> Result<()> {
    let dir = work_dir()?;
    let cryo_state =
        state::load_state(&state_path(&dir))?.context("No cryochamber instance found.")?;

    let timer_impl = timer::create_timer()?;

    if let Some(wake_id) = &cryo_state.wake_timer_id {
        timer_impl.cancel(&timer::TimerId(wake_id.clone()))?;
        println!("Cancelled wake timer: {wake_id}");
    }
    if let Some(fb_id) = &cryo_state.fallback_timer_id {
        timer_impl.cancel(&timer::TimerId(fb_id.clone()))?;
        println!("Cancelled fallback timer: {fb_id}");
    }

    let sp = state_path(&dir);
    if sp.exists() {
        std::fs::remove_file(sp)?;
    }

    println!("Cryochamber cancelled.");
    Ok(())
}

fn cmd_validate() -> Result<()> {
    let dir = work_dir()?;
    let log = log_path(&dir);

    let latest =
        cryochamber::log::read_latest_session(&log)?.context("No sessions found in log.")?;
    let markers = marker::parse_markers(&latest)?;
    let result = validate::validate_markers(&markers);

    if result.plan_complete {
        println!("Plan is complete. No validation needed.");
        return Ok(());
    }

    for error in &result.errors {
        println!("ERROR: {error}");
    }
    for warning in &result.warnings {
        println!("WARN:  {warning}");
    }

    if result.can_hibernate {
        println!("\nAll checks passed. Ready to hibernate.");
    } else {
        println!("\nValidation FAILED. Cannot hibernate.");
    }
    Ok(())
}

fn cmd_log() -> Result<()> {
    let dir = work_dir()?;
    let log = log_path(&dir);
    if log.exists() {
        let contents = std::fs::read_to_string(log)?;
        println!("{contents}");
    } else {
        println!("No log file found.");
    }
    Ok(())
}

fn gh_sync_path(dir: &Path) -> PathBuf {
    dir.join("gh-sync.json")
}

fn cmd_gh_init(repo: &str, title: Option<&str>) -> Result<()> {
    let dir = work_dir()?;

    let (owner, repo_name) = repo
        .split_once('/')
        .context("--repo must be in 'owner/repo' format")?;

    let default_title = format!(
        "[Cryo] {}",
        dir.file_name().unwrap_or_default().to_string_lossy()
    );
    let title = title.unwrap_or(&default_title);

    // Read plan.md for the Discussion body if it exists
    let plan_content = std::fs::read_to_string(dir.join("plan.md")).unwrap_or_default();
    let body = if plan_content.is_empty() {
        "Cryochamber sync Discussion.".to_string()
    } else {
        format!("## Cryochamber Plan\n\n{plan_content}")
    };

    println!("Creating GitHub Discussion in {repo}...");
    let (node_id, number) =
        cryochamber::channel::github::create_discussion(owner, repo_name, title, &body)?;
    println!("Created Discussion #{number}");

    let self_login = cryochamber::channel::github::whoami().ok();

    let sync_state = cryochamber::gh_sync::GhSyncState {
        repo: repo.to_string(),
        discussion_number: number,
        discussion_node_id: node_id,
        last_read_cursor: None,
        self_login,
        last_pushed_session: None,
    };
    cryochamber::gh_sync::save_sync_state(&gh_sync_path(&dir), &sync_state)?;
    println!("Saved gh-sync.json");

    Ok(())
}

fn cmd_gh_pull() -> Result<()> {
    let dir = work_dir()?;
    let mut sync_state = cryochamber::gh_sync::load_sync_state(&gh_sync_path(&dir))?
        .context("gh-sync.json not found. Run 'cryochamber gh init' first.")?;

    let (owner, repo) = sync_state.owner_repo()?;

    println!(
        "Pulling comments from Discussion #{}...",
        sync_state.discussion_number
    );
    let new_cursor = cryochamber::channel::github::pull_comments(
        owner,
        repo,
        sync_state.discussion_number,
        sync_state.last_read_cursor.as_deref(),
        sync_state.self_login.as_deref(),
        &dir,
    )?;

    if let Some(cursor) = new_cursor {
        sync_state.last_read_cursor = Some(cursor);
        cryochamber::gh_sync::save_sync_state(&gh_sync_path(&dir), &sync_state)?;
    }

    let inbox = cryochamber::message::read_inbox(&dir)?;
    println!("Inbox: {} message(s)", inbox.len());

    Ok(())
}

fn cmd_gh_push() -> Result<()> {
    let dir = work_dir()?;
    let mut sync_state = cryochamber::gh_sync::load_sync_state(&gh_sync_path(&dir))?
        .context("gh-sync.json not found. Run 'cryochamber gh init' first.")?;

    let log = log_path(&dir);
    let latest = cryochamber::log::read_latest_session(&log)?;

    let Some(session_output) = latest else {
        println!("No session log found. Nothing to push.");
        return Ok(());
    };

    let markers = cryochamber::marker::parse_markers(&session_output)?;

    // Read session number from state if available
    let session_num = state::load_state(&state_path(&dir))?
        .map(|s| s.session_number)
        .unwrap_or(0);

    // Skip if this session was already pushed
    if sync_state.last_pushed_session == Some(session_num) {
        println!("Session {session_num} already pushed. Skipping.");
        return Ok(());
    }

    // Build session summary
    let exit_str = markers
        .exit_code
        .as_ref()
        .map(|c| format!("{}", c.as_code()))
        .unwrap_or_else(|| "?".to_string());
    let summary = markers.exit_summary.as_deref().unwrap_or("");
    let plan_note = markers.plan_note.as_deref().unwrap_or("(none)");
    let wake_str = markers
        .wake_time
        .as_ref()
        .map(|w| w.0.format("%Y-%m-%dT%H:%M").to_string())
        .unwrap_or_else(|| "plan complete".to_string());

    let auto_summary = format!(
        "## Session {session_num} Summary\n- Exit: {exit_str} {summary}\n- Plan: {plan_note}\n- Next wake: {wake_str}"
    );

    println!(
        "Posting session summary to Discussion #{}...",
        sync_state.discussion_number
    );
    cryochamber::channel::github::post_comment(&sync_state.discussion_node_id, &auto_summary)?;

    // Post each CRYO:REPLY marker as a separate comment
    for reply in &markers.replies {
        println!("Posting reply...");
        cryochamber::channel::github::post_comment(&sync_state.discussion_node_id, reply)?;
    }

    // Record that this session was pushed
    sync_state.last_pushed_session = Some(session_num);
    cryochamber::gh_sync::save_sync_state(&gh_sync_path(&dir), &sync_state)?;

    println!("Push complete.");
    Ok(())
}

fn cmd_gh_status() -> Result<()> {
    let dir = work_dir()?;
    match cryochamber::gh_sync::load_sync_state(&gh_sync_path(&dir))? {
        None => println!("GitHub sync not configured. Run 'cryochamber gh init' first."),
        Some(state) => {
            println!("Repo: {}", state.repo);
            println!("Discussion: #{}", state.discussion_number);
            println!(
                "Last read cursor: {}",
                state
                    .last_read_cursor
                    .as_deref()
                    .unwrap_or("(none — will read all)")
            );
        }
    }
    Ok(())
}

fn cmd_time(offset: Option<&str>) -> Result<()> {
    let now = chrono::Local::now();

    match offset {
        None => {
            println!("{}", now.format("%Y-%m-%dT%H:%M"));
        }
        Some(s) => {
            let duration = parse_offset(s)?;
            let future = now + duration;
            println!("{}", future.format("%Y-%m-%dT%H:%M"));
        }
    }
    Ok(())
}

/// Parse a relative time offset like "+1 day", "+2 hours", "+30 minutes".
/// Accepts singular and plural forms, with or without the "+" prefix.
fn parse_offset(s: &str) -> Result<chrono::Duration> {
    let s = s.trim().strip_prefix('+').unwrap_or(s).trim();

    let (num_str, unit) = s
        .split_once(char::is_whitespace)
        .context("Expected format: \"+N unit\" (e.g. \"+1 day\", \"+2 hours\")")?;

    let n: i64 = num_str
        .trim()
        .parse()
        .context(format!("Invalid number: {num_str}"))?;

    let unit = unit.trim().to_lowercase();
    let days = |factor: i64| -> Result<chrono::Duration> {
        n.checked_mul(factor)
            .and_then(chrono::Duration::try_days)
            .context(format!("Offset too large: {n} {unit}"))
    };
    let duration = match unit.as_str() {
        "minute" | "minutes" | "min" | "mins" | "m" => {
            chrono::Duration::try_minutes(n).context(format!("Offset too large: {n} {unit}"))?
        }
        "hour" | "hours" | "hr" | "hrs" | "h" => {
            chrono::Duration::try_hours(n).context(format!("Offset too large: {n} {unit}"))?
        }
        "day" | "days" | "d" => {
            chrono::Duration::try_days(n).context(format!("Offset too large: {n} {unit}"))?
        }
        "week" | "weeks" | "w" => {
            chrono::Duration::try_weeks(n).context(format!("Offset too large: {n} {unit}"))?
        }
        "month" | "months" => days(30)?,
        "year" | "years" | "y" => days(365)?,
        _ => anyhow::bail!(
            "Unknown time unit: {unit}. Use minutes, hours, days, weeks, months, or years."
        ),
    };

    Ok(duration)
}
