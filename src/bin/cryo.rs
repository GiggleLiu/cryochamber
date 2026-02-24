// src/bin/cryo.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use cryochamber::message;
use cryochamber::protocol;
use cryochamber::session;
use cryochamber::state::{self, CryoState};

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
        /// Maximum session duration in seconds (0 = no timeout, default: no timeout)
        #[arg(long, default_value = "0")]
        max_session_duration: u64,
        /// Disable inbox file watching
        #[arg(long)]
        no_watch: bool,
    },
    /// Show current status: next wake time, last result
    Status,
    /// List all running cryo daemon processes on this machine
    Ps {
        /// Kill all listed daemons
        #[arg(long)]
        kill_all: bool,
    },
    /// Kill the running daemon and restart it
    Restart,
    /// Stop the daemon and remove state
    Cancel,
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { agent } => cmd_init(&agent),
        Commands::Start {
            plan,
            agent,
            max_retries,
            max_session_duration,
            no_watch,
        } => cmd_start(
            plan.as_deref(),
            &agent,
            max_retries,
            max_session_duration,
            no_watch,
        ),
        Commands::Status => cmd_status(),
        Commands::Ps { kill_all } => cmd_ps(kill_all),
        Commands::Restart => cmd_restart(),
        Commands::Cancel => cmd_cancel(),
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
            let dir = cryochamber::work_dir()?;
            let fb = cryochamber::fallback::FallbackAction {
                action,
                target,
                message,
            };
            fb.execute(&dir)
        }
    }
}

fn cmd_init(agent_cmd: &str) -> Result<()> {
    let dir = cryochamber::work_dir()?;

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

/// Check that the agent command is supported and the binary exists on PATH.
fn validate_agent_command(agent_cmd: &str) -> Result<()> {
    let program = cryochamber::agent::agent_program(agent_cmd)?;
    let status = std::process::Command::new("which")
        .arg(&program)
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
    max_session_duration: u64,
    no_watch: bool,
) -> Result<()> {
    // Validate agent command is supported and binary exists
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

    let dir = cryochamber::work_dir()?;

    // Guard: refuse to start if an instance is already active
    if let Some(existing) = state::load_state(&state::state_path(&dir))? {
        if state::is_locked(&existing) {
            anyhow::bail!(
                "A cryochamber session is already running (PID: {:?}). Use `cryo cancel` to stop it first.",
                existing.pid
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

    // Daemon mode: save state and spawn `cryo daemon` in background
    let cryo_state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 0, // daemon will increment to 1
        last_command: Some(agent_cmd.to_string()),
        pid: None, // no PID lock — daemon will set its own
        max_retries,
        retry_count: 0,
        max_session_duration,
        watch_inbox: !no_watch,
        daemon_mode: true,
    };
    state::save_state(&state::state_path(&dir), &cryo_state)?;

    cryochamber::process::spawn_daemon(&dir)?;

    println!("Cryochamber started (daemon running in background).");
    println!("Use `cryo watch` to follow progress.");
    println!("Use `cryo status` to check state.");

    Ok(())
}

fn cmd_daemon() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let daemon = cryochamber::daemon::Daemon::new(dir);
    daemon.run()
}

fn cmd_status() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let cryo_state = state::load_state(&state::state_path(&dir))?;

    match cryo_state {
        None => println!("No cryochamber instance in this directory."),
        Some(state) => {
            println!("Plan: {}", state.plan_path);
            println!("Session: {}", state.session_number);
            println!(
                "PID: {}",
                state.pid.map(|p| p.to_string()).unwrap_or("none".into())
            );
            println!(
                "Daemon mode: {}",
                if state.daemon_mode { "yes" } else { "no" }
            );
            if state.max_session_duration > 0 {
                println!("Session timeout: {}s", state.max_session_duration);
            }

            let log = cryochamber::log::log_path(&dir);
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
    let dir = cryochamber::work_dir()?;
    let cryo_state =
        state::load_state(&state::state_path(&dir))?.context("No cryochamber instance found.")?;

    // Kill existing daemon process if running
    if let Some(pid) = cryo_state.pid {
        if state::is_locked(&cryo_state) {
            cryochamber::process::terminate_pid(pid)?;
        }
    }

    // Clear PID, keep session_number and last_command
    let updated = CryoState {
        pid: None,
        daemon_mode: false,
        ..cryo_state
    };
    state::save_state(&state::state_path(&dir), &updated)?;

    cryochamber::process::spawn_daemon(&dir)?;

    println!("Restarted. Daemon running in background.");
    println!("Use `cryo watch` to follow progress.");
    Ok(())
}

fn cmd_ps(kill_all: bool) -> Result<()> {
    // list() auto-cleans dead PIDs from the registry
    let entries = cryochamber::registry::list()?;

    if entries.is_empty() {
        println!("No cryo daemons running.");
        return Ok(());
    }

    for entry in &entries {
        println!("PID {:>6}  {}", entry.pid, entry.dir);

        if kill_all {
            cryochamber::process::terminate_pid(entry.pid)?;
        }
    }

    Ok(())
}

fn cmd_cancel() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let cryo_state =
        state::load_state(&state::state_path(&dir))?.context("No cryochamber instance found.")?;

    // Kill daemon process if running
    if let Some(pid) = cryo_state.pid {
        if state::is_locked(&cryo_state) {
            cryochamber::process::terminate_pid(pid)?;
        }
    }

    let sp = state::state_path(&dir);
    if sp.exists() {
        std::fs::remove_file(sp)?;
    }

    println!("Cryochamber cancelled.");
    Ok(())
}

fn cmd_log() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let log = cryochamber::log::log_path(&dir);
    if log.exists() {
        let contents = std::fs::read_to_string(log)?;
        println!("{contents}");
    } else {
        println!("No log file found.");
    }
    Ok(())
}

fn cmd_send(body: &str, from: &str, subject: Option<&str>) -> Result<()> {
    let dir = cryochamber::work_dir()?;
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
    let dir = cryochamber::work_dir()?;
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

    let dir = cryochamber::work_dir()?;
    let log = cryochamber::log::log_path(&dir);
    let state_file = state::state_path(&dir);

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
            } else if !st.daemon_mode {
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
