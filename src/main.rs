// src/main.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use cryochamber::agent;
use cryochamber::log;
use cryochamber::message;
use cryochamber::protocol;
use cryochamber::session::{self, SessionOutcome};
use cryochamber::state::{self, CryoState};
use cryochamber::timer;

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
        /// Path to plan file or directory containing plan.md (default: current directory)
        plan: Option<PathBuf>,
        /// Agent command to use (default: opencode)
        #[arg(long, default_value = "opencode")]
        agent: String,
        /// Max retry attempts on agent spawn failure (default: 1 = no retry)
        #[arg(long, default_value = "1")]
        max_retries: u32,
        /// Run in foreground (block until session completes instead of daemonizing)
        #[arg(long)]
        foreground: bool,
        /// Maximum session duration in seconds (0 = no timeout, default: 1800)
        #[arg(long, default_value = "1800")]
        max_session_duration: u64,
        /// Disable inbox file watching
        #[arg(long)]
        no_watch: bool,
    },
    /// Called by OS timer: execute the next scheduled task
    Wake {
        /// Force wake immediately (cancel existing timer)
        #[arg(long)]
        now: bool,
    },
    /// Show current status: next wake time, last result
    Status,
    /// Cancel current timers and immediately run the next session
    Restart,
    /// Cancel all timers and stop the schedule
    Cancel,
    /// Run pre-hibernate validation checks
    Validate,
    /// Print the session log
    Log,
    /// Watch the session log in real-time
    Watch {
        /// Show full log from the beginning (default: start from current position)
        #[arg(long)]
        all: bool,
    },
    /// Send a message to the agent's inbox
    Send {
        /// Message body
        body: String,
        /// Sender name (default: "human")
        #[arg(long, default_value = "human")]
        from: String,
        /// Message subject (default: derived from body)
        #[arg(long)]
        subject: Option<String>,
    },
    /// Read messages from the agent's outbox
    Receive,
    /// Execute a fallback action (used internally by timers)
    FallbackExec {
        action: String,
        target: String,
        message: String,
    },
    /// Run the persistent daemon (internal — use `cryo start` instead)
    #[command(hide = true)]
    Daemon,
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
            foreground,
            max_session_duration,
            no_watch,
        } => cmd_start(
            plan.as_deref(),
            &agent,
            max_retries,
            foreground,
            max_session_duration,
            no_watch,
        ),
        Commands::Wake { now } => cmd_wake(now),
        Commands::Status => cmd_status(),
        Commands::Restart => cmd_restart(),
        Commands::Cancel => cmd_cancel(),
        Commands::Validate => cmd_validate(),
        Commands::Log => cmd_log(),
        Commands::Watch { all } => cmd_watch(all),
        Commands::Send {
            body,
            from,
            subject,
        } => cmd_send(&body, &from, subject.as_deref()),
        Commands::Daemon => cmd_daemon(),
        Commands::Receive => cmd_receive(),
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

    if protocol::write_makefile(&dir)? {
        println!("Wrote Makefile (agent utilities)");
    } else {
        println!("Makefile already exists, skipping");
    }

    message::ensure_dirs(&dir)?;
    println!("Created messages/ directory");

    println!("\nCryochamber initialized. Next steps:");
    println!("  1. Edit plan.md with your task plan");
    println!("  2. Run: cryo start");

    Ok(())
}

/// Check that the agent command binary exists on PATH before spawning.
fn validate_agent_command(agent_cmd: &str) -> Result<()> {
    let parts =
        shell_words::split(agent_cmd).context("Failed to parse agent command")?;
    let program = parts.first().context("Agent command is empty")?;
    let status = std::process::Command::new("which")
        .arg(program)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => anyhow::bail!(
            "Agent command '{}' not found. Verify it is installed and on your PATH.",
            program
        ),
    }
}

fn cmd_start(
    plan_path: Option<&Path>,
    agent_cmd: &str,
    max_retries: u32,
    foreground: bool,
    max_session_duration: u64,
    no_watch: bool,
) -> Result<()> {
    // Validate agent command exists before doing anything else
    validate_agent_command(agent_cmd)?;

    // Resolve plan path: None => "plan.md" in cwd, Some(dir) => cd into dir, Some(file) => use file
    let plan_path = match plan_path {
        None => PathBuf::from("plan.md"),
        Some(p) if p.is_dir() => {
            std::env::set_current_dir(p)
                .with_context(|| format!("Failed to cd into {}", p.display()))?;
            PathBuf::from("plan.md")
        }
        Some(p) => p.to_path_buf(),
    };
    let plan_path = plan_path.as_path();

    let dir = work_dir()?;

    // Guard: refuse to start if an instance is already active
    if let Some(existing) = state::load_state(&state_path(&dir))? {
        if state::is_locked(&existing) {
            anyhow::bail!(
                "A cryochamber session is already running (PID: {:?}). Use `cryo cancel` to stop it first.",
                existing.pid
            );
        }
        if existing.wake_timer_id.is_some() {
            anyhow::bail!(
                "A cryochamber instance already exists with a pending wake timer. Use `cryo cancel` to stop it first."
            );
        }
    }

    // Auto-init: ensure the agent-specific protocol file and message dirs exist
    let filename = protocol::protocol_filename(agent_cmd);
    if protocol::write_protocol_file(&dir, filename)? {
        println!("Wrote {filename} (cryochamber protocol)");
    }
    protocol::write_makefile(&dir)?;
    message::ensure_dirs(&dir)?;

    let plan_dest = dir.join("plan.md");

    if session::should_copy_plan(plan_path, &plan_dest) {
        std::fs::copy(plan_path, &plan_dest).context("Failed to copy plan file")?;
    }

    if foreground {
        // Legacy blocking mode: run session in the foreground
        let mut cryo_state = CryoState {
            plan_path: "plan.md".to_string(),
            session_number: 1,
            last_command: Some(agent_cmd.to_string()),
            wake_timer_id: None,
            fallback_timer_id: None,
            pid: Some(std::process::id()),
            max_retries,
            retry_count: 0,
            max_session_duration,
            watch_inbox: !no_watch,
            daemon_mode: false,
        };
        state::save_state(&state_path(&dir), &cryo_state)?;

        println!("Cryochamber initialized. Running first task (foreground)...");

        run_session(
            &dir,
            &mut cryo_state,
            agent_cmd,
            "Execute the first task from the plan",
        )?;
    } else {
        // Daemon mode: save state and spawn `cryo daemon` in background
        let cryo_state = CryoState {
            plan_path: "plan.md".to_string(),
            session_number: 0, // daemon will increment to 1
            last_command: Some(agent_cmd.to_string()),
            wake_timer_id: None,
            fallback_timer_id: None,
            pid: None, // no PID lock — daemon will set its own
            max_retries,
            retry_count: 0,
            max_session_duration,
            watch_inbox: !no_watch,
            daemon_mode: true,
        };
        state::save_state(&state_path(&dir), &cryo_state)?;

        let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
        let daemon_out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("cryo.log"))
            .context("Failed to open cryo.log for daemon output")?;
        let daemon_err = daemon_out
            .try_clone()
            .context("Failed to clone log handle")?;
        std::process::Command::new(&exe)
            .arg("daemon")
            .current_dir(&dir)
            .stdin(std::process::Stdio::null())
            .stdout(daemon_out)
            .stderr(daemon_err)
            .spawn()
            .context("Failed to spawn daemon process")?;

        println!("Cryochamber started (daemon running in background).");
        println!("Use `cryo watch` to follow progress.");
        println!("Use `cryo status` to check state.");
    }

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
    println!("Session #{}: Running agent...", cryo_state.session_number);

    let max_retries = cryo_state.max_retries;
    let agent_cmd_owned = agent_cmd.to_string();
    let result = session::execute_session(
        dir,
        cryo_state.session_number,
        task,
        log,
        |prompt, writer| run_agent_with_retry(max_retries, &agent_cmd_owned, prompt, writer),
    )?;

    cryo_state.retry_count = 0;

    for warning in &result.warnings {
        eprintln!("Warning: {warning}");
    }

    match result.outcome {
        SessionOutcome::PlanComplete => {
            println!("Plan complete! No more wake-ups scheduled.");
        }
        SessionOutcome::ValidationFailed { errors, .. } => {
            for error in &errors {
                eprintln!("Error: {error}");
            }
            anyhow::bail!("Pre-hibernate validation failed. Not hibernating.");
        }
        SessionOutcome::Hibernate {
            wake_time,
            fallback,
            command,
        } => {
            let timer_impl = timer::create_timer()?;
            let dir_str = dir.to_string_lossy().to_string();
            let wake_cmd = format!("{} wake", std::env::current_exe()?.to_string_lossy());

            let wake_id = timer_impl.schedule_wake(wake_time, &wake_cmd, &dir_str)?;
            cryo_state.wake_timer_id = Some(wake_id.0.clone());

            if let Some(cmd) = &command {
                cryo_state.last_command = Some(cmd.clone());
            }

            if let Some(fb) = &fallback {
                let fallback_time = wake_time + chrono::Duration::hours(1);
                let fb_id = timer_impl.schedule_fallback(fallback_time, fb, &dir_str)?;
                cryo_state.fallback_timer_id = Some(fb_id.0.clone());
            }

            let status =
                timer_impl.verify(&timer::TimerId(cryo_state.wake_timer_id.clone().unwrap()))?;
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
        }
    }

    Ok(())
}

/// Attempt to run the agent, retrying on spawn failure up to `max_retries` times.
/// Retry failures are written through the session writer (not as separate sessions)
/// to keep one contiguous session block in the log.
fn run_agent_with_retry(
    max_retries: u32,
    agent_cmd: &str,
    prompt: &str,
    writer: &mut log::SessionWriter,
) -> Result<agent::AgentResult> {
    let mut last_err = String::new();

    for attempt in 1..=max_retries {
        match agent::run_agent_streaming(agent_cmd, prompt, Some(writer)) {
            Ok(r) => return Ok(r),
            Err(e) => {
                last_err =
                    format!("Agent failed to run (attempt {attempt}/{max_retries}): {e}");
                eprintln!("{last_err}");
                let _ = writer.write_line(&format!("[retry] {last_err}"));

                if attempt < max_retries {
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
    session::derive_task_from_output(&latest)
}

fn cmd_daemon() -> Result<()> {
    let dir = work_dir()?;
    let daemon = cryochamber::daemon::Daemon::new(dir);
    daemon.run()
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
            println!(
                "Daemon mode: {}",
                if state.daemon_mode { "yes" } else { "no" }
            );
            if state.max_session_duration > 0 {
                println!("Session timeout: {}s", state.max_session_duration);
            }

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

fn cmd_restart() -> Result<()> {
    let dir = work_dir()?;
    let cryo_state =
        state::load_state(&state_path(&dir))?.context("No cryochamber instance found.")?;

    // Kill existing process (daemon or one-shot session)
    if let Some(pid) = cryo_state.pid {
        if state::is_locked(&cryo_state) {
            println!("Killing process (PID: {pid})...");
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // Cancel existing OS timers (non-daemon mode)
    let timer_impl = timer::create_timer()?;
    if let Some(wake_id) = &cryo_state.wake_timer_id {
        let _ = timer_impl.cancel(&timer::TimerId(wake_id.clone()));
    }
    if let Some(fb_id) = &cryo_state.fallback_timer_id {
        let _ = timer_impl.cancel(&timer::TimerId(fb_id.clone()));
    }

    // Clear timers and PID, keep session_number and last_command
    let updated = CryoState {
        wake_timer_id: None,
        fallback_timer_id: None,
        pid: None,
        daemon_mode: false,
        ..cryo_state
    };
    state::save_state(&state_path(&dir), &updated)?;

    // Spawn daemon in background
    let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("cryo.log"))
        .context("Failed to open cryo.log")?;
    let err_file = log_file.try_clone().context("Failed to clone log handle")?;
    std::process::Command::new(&exe)
        .arg("daemon")
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(err_file)
        .spawn()
        .context("Failed to spawn daemon")?;

    println!("Restarted. Daemon running in background.");
    println!("Use `cryo watch` to follow progress.");
    Ok(())
}

fn cmd_cancel() -> Result<()> {
    let dir = work_dir()?;
    let cryo_state =
        state::load_state(&state_path(&dir))?.context("No cryochamber instance found.")?;

    // Kill daemon/session process if running
    if let Some(pid) = cryo_state.pid {
        if state::is_locked(&cryo_state) {
            println!("Sending SIGTERM to process {pid}...");
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            // Wait for clean exit
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    // Cancel OS timers (for non-daemon mode)
    let timer_impl = timer::create_timer()?;
    if let Some(wake_id) = &cryo_state.wake_timer_id {
        let _ = timer_impl.cancel(&timer::TimerId(wake_id.clone()));
        println!("Cancelled wake timer: {wake_id}");
    }
    if let Some(fb_id) = &cryo_state.fallback_timer_id {
        let _ = timer_impl.cancel(&timer::TimerId(fb_id.clone()));
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
    let markers = cryochamber::marker::parse_markers(&latest)?;
    let result = cryochamber::validate::validate_markers(&markers);

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

fn cmd_send(body: &str, from: &str, subject: Option<&str>) -> Result<()> {
    let dir = work_dir()?;
    message::ensure_dirs(&dir)?;

    let subject = subject
        .map(|s| s.to_string())
        .unwrap_or_else(|| body.chars().take(50).collect::<String>());

    let msg = message::Message {
        from: from.to_string(),
        subject,
        body: body.to_string(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: std::collections::BTreeMap::new(),
    };

    let path = message::write_message(&dir, "inbox", &msg)?;
    println!(
        "Message sent to {}",
        path.strip_prefix(&dir).unwrap_or(&path).display()
    );
    Ok(())
}

fn cmd_receive() -> Result<()> {
    let dir = work_dir()?;
    let messages = message::read_outbox(&dir)?;

    if messages.is_empty() {
        println!("No messages in outbox.");
        return Ok(());
    }

    for (filename, msg) in &messages {
        println!("--- {} ---", filename);
        println!("From: {}", msg.from);
        println!("Subject: {}", msg.subject);
        println!("Time: {}", msg.timestamp.format("%Y-%m-%dT%H:%M:%S"));
        println!();
        println!("{}", msg.body);
        println!();
    }

    Ok(())
}

fn cmd_watch(show_all: bool) -> Result<()> {
    use std::io::Read;

    let dir = work_dir()?;
    let log = log_path(&dir);
    let state_file = state_path(&dir);

    if !log.exists() {
        println!("Waiting for first session output...");
    }

    // Start from end of file unless --all
    let mut pos = if show_all {
        0
    } else {
        log.metadata().map(|m| m.len()).unwrap_or(0)
    };

    let mut no_state_ticks: u32 = 0;

    loop {
        // Read new content from the log file
        if log.exists() {
            let file_len = log.metadata().map(|m| m.len()).unwrap_or(0);
            if file_len > pos {
                let mut f = std::fs::File::open(&log)?;
                std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(pos))?;
                let mut buf = String::new();
                f.read_to_string(&mut buf)?;
                print!("{buf}");
                pos = file_len;
                no_state_ticks = 0; // reset grace period on new output
            }
        }

        // Check if a session is currently active
        if let Some(st) = state::load_state(&state_file)? {
            no_state_ticks = 0;
            if state::is_locked(&st) {
                // Session is running, keep polling
            } else if st.wake_timer_id.is_none() && !st.daemon_mode {
                // Final drain
                if log.exists() {
                    let file_len = log.metadata().map(|m| m.len()).unwrap_or(0);
                    if file_len > pos {
                        let mut f = std::fs::File::open(&log)?;
                        std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(pos))?;
                        let mut buf = String::new();
                        f.read_to_string(&mut buf)?;
                        print!("{buf}");
                    }
                }
                println!("\n(No active session or pending timer. Exiting watch.)");
                break;
            }
        } else {
            no_state_ticks += 1;
            // 500ms * 20 = 10s grace period
            if no_state_ticks >= 20 {
                println!("\n(No cryochamber instance found. Exiting watch.)");
                break;
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
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

    let auto_summary = session::format_session_summary(session_num, &markers);

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
