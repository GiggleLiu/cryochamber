// src/bin/cryo.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::Path;

use cryochamber::channel::store::MessageStore;
use cryochamber::config;
use cryochamber::lifecycle::{self, DaemonLaunchMode, StartOptions};
use cryochamber::message::Message;
use cryochamber::state::{self, CryoState};

#[derive(Parser)]
#[command(name = "cryo", about = "Long-term AI agent task scheduler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a working directory with config and template plan
    Init {
        /// Agent command to store in cryo.toml
        #[arg(long, default_value = "opencode")]
        agent: String,
    },
    /// Begin a new plan: initialize and run the first task
    Start {
        /// Agent command to use (overrides cryo.toml)
        #[arg(long)]
        agent: Option<String>,
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
    /// Stop the daemon and remove all runtime files (confirms first)
    Clean {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Print the session log
    Log,
    /// Watch the session log in real-time
    Watch {
        /// Show full log from the beginning (default: start from current position)
        #[arg(long)]
        all: bool,
        /// Which log to follow: "cryo" for structured events, "agent" for raw agent output
        #[arg(long, default_value = "cryo")]
        viewpoint: String,
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
        /// Wake the agent immediately after sending
        #[arg(long)]
        wake: bool,
    },
    /// Read messages from the agent's outbox
    Receive,
    /// Send a wake message to the daemon's inbox
    Wake {
        /// Message to include in the agent's prompt
        message: Option<String>,
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
            max_session_duration,
        } => cmd_start(agent, max_session_duration),
        Commands::Status => cmd_status(),
        Commands::Ps { kill_all } => cmd_ps(kill_all),
        Commands::Restart => cmd_restart(),
        Commands::Cancel => cmd_cancel(),
        Commands::Clean { force } => cmd_clean(force),
        Commands::Log => cmd_log(),
        Commands::Watch { all, viewpoint } => cmd_watch(all, &viewpoint),
        Commands::Send {
            body,
            from,
            subject,
            wake,
        } => cmd_send(&body, &from, subject.as_deref(), wake),
        Commands::Wake { message } => cmd_wake(message.as_deref()),
        Commands::Daemon => cmd_daemon(),
        Commands::Receive => cmd_receive(),
    }
}

fn cmd_init(agent_cmd: &str) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let report = cryochamber::protocol::scaffold_chamber(&dir, agent_cmd)?;

    let line = |label: &str, created: bool| {
        if created {
            println!("  {label} (created)");
        } else {
            println!("  {label} (exists, kept)");
        }
    };
    line("cryo.toml", report.cryo_toml_created);
    line("plan.md", report.plan_created);
    line("README.md", report.readme_created);
    line("NOTES.md", report.notes_created);

    println!("\nCryochamber initialized. Next steps:");
    println!("  1. Edit plan.md with your task plan");
    println!("  2. Run: cryo start");

    Ok(())
}

fn daemon_responding(dir: &Path) -> bool {
    lifecycle::daemon_responding(dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonTerminationAction {
    Terminate(u32),
    Skip,
}

fn daemon_termination_action(
    state_locked: bool,
    daemon_responding: bool,
    pid: Option<u32>,
) -> DaemonTerminationAction {
    match (state_locked, daemon_responding, pid) {
        (true, true, Some(pid)) => DaemonTerminationAction::Terminate(pid),
        _ => DaemonTerminationAction::Skip,
    }
}

fn terminate_daemon_if_reachable(dir: &Path, cryo_state: &CryoState) -> Result<()> {
    let state_locked = state::is_locked(cryo_state);
    let responding = state_locked && daemon_responding(dir);
    match daemon_termination_action(state_locked, responding, cryo_state.pid) {
        DaemonTerminationAction::Terminate(pid) => {
            cryochamber::process::terminate_pid(pid)?;
            println!("Killed daemon (PID {pid}).");
        }
        DaemonTerminationAction::Skip => {}
    }
    Ok(())
}

fn cmd_start(
    agent_override: Option<String>,
    max_session_duration_override: Option<u64>,
) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let prepared = lifecycle::prepare_start(
        &dir,
        StartOptions {
            agent_override,
            max_session_duration_override,
        },
    )?;

    // Validate agent command using effective agent value
    let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
    lifecycle::validate_agent_command(&prepared.effective_agent, exe.parent())?;

    // Ensure message dirs exist (needed for inbox watching)
    MessageStore::new(dir.clone()).ensure_dirs()?;

    state::save_state(&state::state_path(&dir), &prepared.state)?;

    let launch_mode = lifecycle::launch_daemon(&dir, &exe)?;
    lifecycle::wait_for_live_daemon(&dir)?;

    match launch_mode {
        DaemonLaunchMode::BackgroundProcess => {
            println!("Cryochamber started (background process).")
        }
        DaemonLaunchMode::Service => {
            println!("Cryochamber started (service installed, survives reboot).")
        }
    }

    println!(
        "Use `cryo watch` or `cryohub start` (from a parent of chamber dirs) to follow progress."
    );
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
    lifecycle::require_valid_project(&dir)?;

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

            // Show next wake time from TODO list
            let todo_file = cryochamber::todo::TodoFile::new(dir.join("todo.json"));
            match todo_file.next_wake_time() {
                Ok(Some(wake)) => println!("Next wake: {wake}"),
                Ok(None) => println!("Next wake: idle (no pending TODOs)"),
                Err(_) => {}
            }

            if let Some(pid) = st.pid {
                println!("PID: {pid}");
            }

            // Config
            let effective_agent = st.agent_override.as_deref().unwrap_or(&cfg.agent);
            println!("Agent: {effective_agent}");
            if st.agent_override.is_some() {
                println!("  (override; cryo.toml has \"{}\")", cfg.agent);
            }
            if let Some(provider) = cfg.active_provider() {
                println!("Provider: {}", provider.name);
                if cfg.uses_legacy_providers() {
                    println!("  (legacy [[providers]]; use [provider])");
                }
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
    lifecycle::require_live_daemon(&dir)?;

    let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
    let launch_mode = lifecycle::restart_chamber(&dir, &exe)?;

    match launch_mode {
        DaemonLaunchMode::BackgroundProcess => println!("Restarted (background process)."),
        DaemonLaunchMode::Service => println!("Restarted (service reinstalled)."),
    }
    println!(
        "Use `cryo watch` or `cryohub start` (from a parent of chamber dirs) to follow progress."
    );
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
    lifecycle::require_valid_project(&dir)?;

    // Uninstall system service (launchd/systemd) if installed
    let service_removed = cryochamber::service::uninstall("daemon", &dir)?;
    if service_removed {
        println!("Service removed.");
    }

    let sp = state::state_path(&dir);
    match state::load_state(&sp)? {
        None => {
            if !service_removed {
                anyhow::bail!("Nothing to cancel. No daemon state or service found.");
            }
        }
        Some(cryo_state) => {
            terminate_daemon_if_reachable(&dir, &cryo_state)?;
            // Always clean up state file
            std::fs::remove_file(sp)?;
            println!("Removed timer.json.");
        }
    }

    println!("Cryochamber cancelled.");
    Ok(())
}

/// Prompt the user for y/n confirmation. Returns true if confirmed.
fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N] ");
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim(), "y" | "Y" | "yes" | "Yes")
}

fn cmd_clean(force: bool) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    lifecycle::require_valid_project(&dir)?;

    if !force && !confirm("Stop daemon and remove all runtime files?") {
        println!("Aborted.");
        return Ok(());
    }

    // Uninstall services (daemon + gh-sync)
    if cryochamber::service::uninstall("daemon", &dir)? {
        println!("Removed daemon service.");
    }
    if cryochamber::service::uninstall("gh-sync", &dir)? {
        println!("Removed gh-sync service.");
    }
    if cryochamber::service::uninstall("zulip-sync", &dir)? {
        println!("Removed zulip-sync service.");
    }
    // `cryohub` is workspace-scoped and stores its service log in user-level
    // Cryo state, not in a chamber dir. `cryo clean` is chamber-scoped, so it
    // cannot and should not touch hub state. Use `cryohub stop` from the
    // workspace directory to remove the hub service.

    // Kill daemon process if still running
    let sp = state::state_path(&dir);
    if let Some(cryo_state) = state::load_state(&sp)? {
        terminate_daemon_if_reachable(&dir, &cryo_state)?;
    }

    // Remove chamber runtime files. Hub logs are user-level Cryo state and are
    // not part of a chamber clean.
    let runtime_files = [
        "timer.json",
        "cryo.log",
        "cryo-agent.log",
        "cryo-gh-sync.log",
        "cryo-gh-sync.pid",
        "cryo-zulip-sync.log",
        "cryo-zulip-sync.pid",
    ];
    for name in &runtime_files {
        let path = dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("Removed {name}");
        }
    }

    // Remove runtime directories. Keep sync configuration such as
    // gh-sync.json, zulip-sync.json, and .cryo/zuliprc.
    let runtime_dirs = ["messages"];
    for name in &runtime_dirs {
        let path = dir.join(name);
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
            println!("Removed {name}/");
        }
    }

    let sock_path = cryochamber::socket::socket_path(&dir);
    if sock_path.exists() {
        std::fs::remove_file(&sock_path)?;
        println!("Removed .cryo/cryo.sock");
    }
    let cryo_dir = dir.join(".cryo");
    if cryo_dir.exists() && cryo_dir.read_dir()?.next().is_none() {
        std::fs::remove_dir(&cryo_dir)?;
        println!("Removed .cryo/");
    }

    println!("Clean.");
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

fn build_inbox_message(from: &str, subject: &str, body: &str) -> Message {
    Message {
        from: from.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: std::collections::BTreeMap::new(),
        is_question: false,
    }
}

/// Check if a daemon is running in the given directory.
fn is_daemon_running(dir: &std::path::Path) -> bool {
    if let Ok(Some(st)) = state::load_state(&state::state_path(dir)) {
        return state::is_locked(&st) && daemon_responding(dir);
    }
    false
}

/// Send SIGUSR1 to the daemon to force an immediate wake.
/// Returns true if the signal was delivered successfully.
fn signal_daemon_wake(dir: &std::path::Path) -> bool {
    cryochamber::daemon_client::signal_daemon_wake(dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WakeNotificationAction {
    QueueUntilStart,
    InboxWatcher,
    SendSignal,
}

fn wake_notification_action(daemon_running: bool, watch_inbox: bool) -> WakeNotificationAction {
    match (daemon_running, watch_inbox) {
        (false, _) => WakeNotificationAction::QueueUntilStart,
        (true, true) => WakeNotificationAction::InboxWatcher,
        (true, false) => WakeNotificationAction::SendSignal,
    }
}

/// After writing an inbox message, notify the daemon and print status.
/// When watch_inbox is true, the inotify watcher handles wake — no signal needed.
/// When watch_inbox is false, send SIGUSR1.
fn notify_daemon_wake(dir: &std::path::Path) -> Result<()> {
    let watch_inbox = config::load_config(&config::config_path(dir))?
        .map(|c| c.watch_inbox)
        .unwrap_or(true);

    match wake_notification_action(is_daemon_running(dir), watch_inbox) {
        WakeNotificationAction::QueueUntilStart => {
            eprintln!("Warning: no daemon is running. Message queued for the next `cryo start`.");
        }
        WakeNotificationAction::InboxWatcher => {
            println!("Daemon will pick it up shortly.");
        }
        WakeNotificationAction::SendSignal => {
            if signal_daemon_wake(dir) {
                println!("Wake signal sent. Daemon waking now.");
            } else {
                eprintln!("Warning: failed to signal daemon. Message queued for the next session.");
            }
        }
    }
    Ok(())
}

fn cmd_wake(wake_message: Option<&str>) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    lifecycle::require_valid_project(&dir)?;
    let store = MessageStore::new(dir.clone());

    let body = wake_message.unwrap_or("Manual wake requested by operator.");
    let msg = build_inbox_message("operator", "Wake", body);
    store.send_in(&msg)?;

    notify_daemon_wake(&dir)
}

fn cmd_send(body: &str, from: &str, subject: Option<&str>, wake: bool) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    lifecycle::require_valid_project(&dir)?;
    let store = MessageStore::new(dir.clone());

    let subject = subject.unwrap_or_else(|| {
        // Truncate at a char boundary to avoid panic on non-ASCII input
        let mut end = body.len().min(50);
        while end > 0 && !body.is_char_boundary(end) {
            end -= 1;
        }
        &body[..end]
    });
    let msg = build_inbox_message(from, subject, body);
    let path = store.send_in(&msg)?;
    println!(
        "Message sent to {}",
        path.strip_prefix(&dir).unwrap_or(&path).display()
    );

    if wake {
        notify_daemon_wake(&dir)?;
    }

    Ok(())
}

fn cmd_receive() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let messages = MessageStore::new(dir).read_outbox_named()?;

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

fn cmd_watch(show_all: bool, viewpoint: &str) -> Result<()> {
    use std::io::Read;

    let dir = cryochamber::work_dir()?;
    lifecycle::require_valid_project(&dir)?;
    let log = match viewpoint {
        "agent" => cryochamber::log::agent_log_path(&dir),
        "cryo" => cryochamber::log::log_path(&dir),
        other => anyhow::bail!("Unknown viewpoint '{other}'. Use 'cryo' or 'agent'."),
    };
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

#[cfg(test)]
#[path = "unit_tests/cryo.rs"]
mod tests;
