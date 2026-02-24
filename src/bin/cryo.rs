// src/bin/cryo.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::Path;

use cryochamber::config;
use cryochamber::message;
use cryochamber::protocol;
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
        /// Agent command to use (overrides cryo.toml)
        #[arg(long)]
        agent: Option<String>,
        /// Max retry attempts on agent spawn failure (overrides cryo.toml)
        #[arg(long)]
        max_retries: Option<u32>,
        /// Maximum session duration in seconds (overrides cryo.toml)
        #[arg(long)]
        max_session_duration: Option<u64>,
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
    /// Send a wake message to the daemon's inbox
    Wake {
        /// Message to include in the agent's prompt
        message: Option<String>,
    },
    /// Execute a fallback action (internal — used by timers)
    #[command(hide = true)]
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
            agent,
            max_retries,
            max_session_duration,
        } => cmd_start(agent, max_retries, max_session_duration),
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
        Commands::Wake { message } => cmd_wake(message.as_deref()),
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

/// Check that this directory is a valid cryo project.
/// Accepts cryo.toml (new) or protocol file (legacy backward compat).
fn require_valid_project(dir: &Path) -> Result<()> {
    if !config::config_path(dir).exists() && protocol::find_protocol_file(dir).is_none() {
        anyhow::bail!("No cryochamber project in this directory. Run `cryo init` first.");
    }
    Ok(())
}

/// Check that a live daemon is running in the current directory.
fn require_live_daemon(dir: &Path) -> Result<CryoState> {
    require_valid_project(dir)?;
    let cryo_state = state::load_state(&state::state_path(dir))?
        .context("No daemon state found. Run `cryo start` first.")?;
    if !state::is_locked(&cryo_state) {
        anyhow::bail!(
            "No live daemon in this directory (stale state from a previous run). \
             Run `cryo start` to start a new one, or `cryo cancel` to clean up stale state."
        );
    }
    Ok(cryo_state)
}

fn cmd_init(agent_cmd: &str) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    // Write cryo.toml first (project config)
    if protocol::write_config_file(&dir, agent_cmd)? {
        println!("Wrote cryo.toml");
    } else {
        eprintln!("Warning: cryo.toml already exists, not replaced");
    }

    let filename = protocol::protocol_filename(agent_cmd);
    if protocol::write_protocol_file(&dir, filename)? {
        println!("Wrote {filename}");
    } else {
        eprintln!("Warning: {filename} already exists, not replaced");
    }

    if protocol::write_template_plan(&dir)? {
        println!("Wrote plan.md");
    } else {
        eprintln!("Warning: plan.md already exists, not replaced");
    }

    if protocol::write_makefile(&dir)? {
        println!("Wrote Makefile");
    } else {
        eprintln!("Warning: Makefile already exists, not replaced");
    }

    message::ensure_dirs(&dir)?;

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
    agent_override: Option<String>,
    max_retries_override: Option<u32>,
    max_session_duration_override: Option<u64>,
) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    // Require init: protocol file or cryo.toml must exist
    require_valid_project(&dir)?;

    // Require plan.md in the working directory
    if !dir.join("plan.md").exists() {
        anyhow::bail!("No plan.md found in the working directory. Create one or run `cryo init`.");
    }

    // Guard: refuse to start if an instance is already active
    if let Some(existing) = state::load_state(&state::state_path(&dir))? {
        if state::is_locked(&existing) {
            anyhow::bail!(
                "A cryochamber session is already running (PID: {:?}). Use `cryo cancel` to stop it first.",
                existing.pid
            );
        }
    }

    // Load config from cryo.toml (fall back to defaults for legacy projects)
    let cfg = config::load_config(&config::config_path(&dir))?.unwrap_or_default();

    // Resolve effective values: CLI override > cryo.toml > hardcoded default
    let effective_agent = agent_override.as_deref().unwrap_or(&cfg.agent);

    // Validate agent command using effective agent value
    validate_agent_command(effective_agent)?;

    // Ensure message dirs exist (needed for inbox watching)
    message::ensure_dirs(&dir)?;

    // Build slim CryoState with override fields only when CLI flags were explicitly provided
    let cryo_state = CryoState {
        session_number: 0, // daemon will increment to 1
        pid: None,         // no PID lock — daemon will set its own
        retry_count: 0,
        agent_override,
        max_retries_override,
        max_session_duration_override,
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
    require_valid_project(&dir)?;

    let cfg = config::load_config(&config::config_path(&dir))?.unwrap_or_default();

    match state::load_state(&state::state_path(&dir))? {
        None => {
            println!("No daemon has been started yet. Run `cryo start` to begin.");
            println!("\nConfig (cryo.toml):");
            println!("  Agent: {}", cfg.agent);
        }
        Some(st) => {
            // Runtime state first
            println!(
                "Daemon: {}",
                if state::is_locked(&st) {
                    "running"
                } else {
                    "stopped"
                }
            );
            println!("Session: {}", st.session_number);
            if let Some(pid) = st.pid {
                println!("PID: {pid}");
            }

            // Config
            let effective_agent = st.agent_override.as_deref().unwrap_or(&cfg.agent);
            println!("Agent: {effective_agent}");
            if st.agent_override.is_some() {
                println!("  (override; cryo.toml has \"{}\")", cfg.agent);
            }
            let effective_timeout = st
                .max_session_duration_override
                .unwrap_or(cfg.max_session_duration);
            if effective_timeout > 0 {
                println!("Session timeout: {effective_timeout}s");
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
    let cryo_state = require_live_daemon(&dir)?;

    // Kill existing daemon process
    if let Some(pid) = cryo_state.pid {
        cryochamber::process::terminate_pid(pid)?;
    }

    // Clear PID, keep session_number and overrides
    let updated = CryoState {
        pid: None,
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
        if kill_all {
            cryochamber::process::terminate_pid(entry.pid)?;
            println!("Killed PID {:>6}  {}", entry.pid, entry.dir);
        } else {
            println!("PID {:>6}  {}", entry.pid, entry.dir);
        }
    }

    Ok(())
}

fn cmd_cancel() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    require_valid_project(&dir)?;

    let sp = state::state_path(&dir);
    match state::load_state(&sp)? {
        None => {
            anyhow::bail!("No daemon state found. Nothing to cancel.");
        }
        Some(cryo_state) => {
            // Kill daemon process if still alive
            if state::is_locked(&cryo_state) {
                if let Some(pid) = cryo_state.pid {
                    cryochamber::process::terminate_pid(pid)?;
                    println!("Killed daemon (PID {pid}).");
                }
            }
            // Always clean up state file
            std::fs::remove_file(sp)?;
            println!("Removed timer.json.");
        }
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

fn build_inbox_message(from: &str, subject: &str, body: &str) -> message::Message {
    message::Message {
        from: from.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: std::collections::BTreeMap::new(),
    }
}

fn cmd_wake(wake_message: Option<&str>) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    require_valid_project(&dir)?;
    message::ensure_dirs(&dir)?;

    // Warn if no daemon is running
    let daemon_alive =
        state::load_state(&state::state_path(&dir))?.is_some_and(|st| state::is_locked(&st));
    if !daemon_alive {
        eprintln!(
            "Warning: no daemon is running. Message will be queued for the next `cryo start`."
        );
    }

    // Warn if watch_inbox is disabled in config
    let cfg = config::load_config(&config::config_path(&dir))?.unwrap_or_default();
    if daemon_alive && !cfg.watch_inbox {
        eprintln!(
            "Warning: inbox watching is disabled in cryo.toml. \
             Message will be delivered at the next scheduled wake, not immediately."
        );
    }

    let body = wake_message.unwrap_or("Manual wake requested by operator.");
    let msg = build_inbox_message("operator", "Wake", body);
    message::write_message(&dir, "inbox", &msg)?;

    if daemon_alive {
        println!("Wake message sent. Daemon will wake on next inbox check.");
    } else {
        println!("Message queued in inbox.");
    }
    Ok(())
}

fn cmd_send(body: &str, from: &str, subject: Option<&str>) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    require_valid_project(&dir)?;
    message::ensure_dirs(&dir)?;

    let subject = subject.unwrap_or_else(|| &body[..body.len().min(50)]);
    let msg = build_inbox_message(from, subject, body);
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
    require_valid_project(&dir)?;
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

        // Check if a daemon is currently running (PID is alive)
        if let Some(st) = state::load_state(&state_file)? {
            no_state_ticks = 0;
            if state::is_locked(&st) {
                // Daemon is running, keep polling
            } else {
                // Daemon has exited — final drain
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
