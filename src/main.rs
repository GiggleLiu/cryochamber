// src/main.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use cryochamber::agent::{self, AgentConfig};
use cryochamber::log::{self, Session};
use cryochamber::marker;
use cryochamber::state::{self, CryoState};
use cryochamber::timer;
use cryochamber::validate;

#[derive(Parser)]
#[command(name = "cryochamber", about = "Long-term AI agent task scheduler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Begin a new plan: initialize and run the first task
    Start {
        /// Path to the natural language plan file
        plan: PathBuf,
        /// Agent command to use (default: opencode)
        #[arg(long, default_value = "opencode")]
        agent: String,
    },
    /// Called by OS timer: execute the next scheduled task
    Wake,
    /// Show current status: next wake time, last result
    Status,
    /// Cancel all timers and stop the schedule
    Cancel,
    /// Run pre-hibernate validation checks
    Validate,
    /// Print the session log
    Log,
    /// Execute a fallback action (used internally by timers)
    FallbackExec {
        action: String,
        target: String,
        message: String,
    },
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
        Commands::Start { plan, agent } => cmd_start(&plan, &agent),
        Commands::Wake => cmd_wake(),
        Commands::Status => cmd_status(),
        Commands::Cancel => cmd_cancel(),
        Commands::Validate => cmd_validate(),
        Commands::Log => cmd_log(),
        Commands::FallbackExec { action, target, message } => {
            let fb = cryochamber::fallback::FallbackAction { action, target, message };
            fb.execute()
        }
    }
}

fn cmd_start(plan_path: &Path, agent_cmd: &str) -> Result<()> {
    let dir = work_dir()?;
    let plan_dest = dir.join("plan.md");

    if plan_path != plan_dest {
        std::fs::copy(plan_path, &plan_dest)
            .context("Failed to copy plan file")?;
    }

    let plan_content = std::fs::read_to_string(&plan_dest)?;

    let mut cryo_state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: Some(agent_cmd.to_string()),
        wake_timer_id: None,
        fallback_timer_id: None,
        pid: Some(std::process::id()),
    };
    state::save_state(&state_path(&dir), &cryo_state)?;

    println!("Cryochamber initialized. Running first task...");

    run_session(&dir, &mut cryo_state, agent_cmd, &plan_content, "Execute the first task from the plan")?;

    Ok(())
}

fn cmd_wake() -> Result<()> {
    let dir = work_dir()?;
    let mut cryo_state = state::load_state(&state_path(&dir))?
        .context("No cryochamber state found. Run 'cryochamber start' first.")?;

    if state::is_locked(&cryo_state) && cryo_state.pid != Some(std::process::id()) {
        anyhow::bail!("Another cryochamber session is running (PID: {:?})", cryo_state.pid);
    }

    cryo_state.pid = Some(std::process::id());
    cryo_state.session_number += 1;
    state::save_state(&state_path(&dir), &cryo_state)?;

    let plan_content = std::fs::read_to_string(dir.join("plan.md"))?;
    let agent_cmd = cryo_state.last_command.clone()
        .unwrap_or_else(|| "opencode".to_string());

    let task = get_task_from_log(&dir).unwrap_or_else(|| "Continue the plan".to_string());

    if let Some(fb_id) = &cryo_state.fallback_timer_id {
        let timer_impl = timer::create_timer()?;
        let _ = timer_impl.cancel(&timer::TimerId(fb_id.clone()));
        cryo_state.fallback_timer_id = None;
    }

    run_session(&dir, &mut cryo_state, &agent_cmd, &plan_content, &task)?;

    Ok(())
}

fn run_session(dir: &Path, cryo_state: &mut CryoState, agent_cmd: &str, plan_content: &str, task: &str) -> Result<()> {
    let log = log_path(dir);

    let log_content = cryochamber::log::read_latest_session(&log)?;
    let config = AgentConfig {
        plan_content: plan_content.to_string(),
        log_content,
        session_number: cryo_state.session_number,
        task: task.to_string(),
    };
    let prompt = agent::build_prompt(&config);

    println!("Session #{}: Running agent...", cryo_state.session_number);
    let result = agent::run_agent(agent_cmd, &prompt)?;

    if result.exit_code != 0 {
        eprintln!("Agent exited with code {}. Stderr:\n{}", result.exit_code, result.stderr);
    }

    let session = Session {
        number: cryo_state.session_number,
        task: task.to_string(),
        output: result.stdout.clone(),
    };
    log::append_session(&log, &session)?;

    let markers = marker::parse_markers(&result.stdout)?;

    let validation = validate::validate_markers(&markers);

    for warning in &validation.warnings {
        eprintln!("Warning: {warning}");
    }

    if validation.plan_complete {
        println!("Plan complete! No more wake-ups scheduled.");
        cryo_state.pid = None;
        state::save_state(&state_path(dir), cryo_state)?;
        return Ok(());
    }

    for error in &validation.errors {
        eprintln!("Error: {error}");
    }

    if !validation.can_hibernate {
        cryo_state.pid = None;
        state::save_state(&state_path(dir), cryo_state)?;
        anyhow::bail!("Pre-hibernate validation failed. Not hibernating.");
    }

    // Schedule next wake
    let timer_impl = timer::create_timer()?;
    // wake_time is Option<WakeTime> — extract the inner NaiveDateTime
    let wake_time = markers.wake_time.unwrap().0;
    let dir_str = dir.to_string_lossy().to_string();

    let wake_cmd = format!(
        "{} wake",
        std::env::current_exe()?.to_string_lossy()
    );

    let wake_id = timer_impl.schedule_wake(wake_time, &wake_cmd, &dir_str)?;
    cryo_state.wake_timer_id = Some(wake_id.0.clone());

    if let Some(cmd) = &markers.command {
        cryo_state.last_command = Some(cmd.clone());
    }

    // Schedule fallback if specified - need to convert marker::FallbackAction to fallback::FallbackAction
    if let Some(fb) = markers.fallbacks.first() {
        let fallback_action = cryochamber::fallback::FallbackAction {
            action: fb.action.clone(),
            target: fb.target.clone(),
            message: fb.message.clone(),
        };
        let fallback_time = wake_time + chrono::Duration::hours(1);
        let fb_id = timer_impl.schedule_fallback(fallback_time, &fallback_action, &dir_str)?;
        cryo_state.fallback_timer_id = Some(fb_id.0.clone());
    }

    // Verify timer
    let status = timer_impl.verify(&timer::TimerId(cryo_state.wake_timer_id.clone().unwrap()))?;
    match status {
        timer::TimerStatus::Scheduled { .. } => {
            println!("Hibernating. Next wake: {}", wake_time.format("%Y-%m-%d %H:%M"));
        }
        timer::TimerStatus::NotFound => {
            anyhow::bail!("Timer registration verification failed!");
        }
    }

    cryo_state.pid = None;
    state::save_state(&state_path(dir), cryo_state)?;

    Ok(())
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
            println!("Wake timer: {}", state.wake_timer_id.as_deref().unwrap_or("none"));
            println!("Fallback timer: {}", state.fallback_timer_id.as_deref().unwrap_or("none"));
            println!("Locked by PID: {}", state.pid.map(|p| p.to_string()).unwrap_or("none".into()));

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
    let cryo_state = state::load_state(&state_path(&dir))?
        .context("No cryochamber instance found.")?;

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

    let latest = cryochamber::log::read_latest_session(&log)?
        .context("No sessions found in log.")?;
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
