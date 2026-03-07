// src/bin/cryo_agent.rs
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::Path;

use cryochamber::message;
use cryochamber::socket::{self, Request};

#[derive(Parser)]
#[command(name = "cryo-agent", about = "Cryochamber agent IPC commands")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// End session and schedule next wake
    Hibernate {
        /// Mark plan as complete (no more wakes)
        #[arg(long)]
        complete: bool,
        /// Exit code: 0=success, 1=partial, 2=failure
        #[arg(long, default_value = "0")]
        exit: u8,
        /// Human-readable session summary
        #[arg(long)]
        summary: Option<String>,
    },
    /// Leave a note for the next session
    Note {
        /// Note text
        text: String,
    },
    /// Send message to human (writes to outbox)
    Send {
        /// Message text
        text: String,
    },
    /// Reply to human (alias for send, writes to outbox)
    Reply {
        /// Reply message text
        text: String,
    },
    /// Set a fallback alert (dead-man switch)
    Alert {
        /// Action type (email, webhook)
        action: String,
        /// Target (email address, URL)
        target: String,
        /// Alert message
        message: String,
    },
    /// Read inbox messages from human
    Receive,
    /// Print current time or compute a future time
    Time {
        /// Offset from now (e.g. "+30 minutes", "+2 hours", "+1 day")
        offset: Option<String>,
        /// Daily time (e.g. "13:00")
        #[arg(long)]
        daily: Option<String>,
    },
    /// Manage TODO items across sessions
    Todo {
        #[command(subcommand)]
        action: TodoAction,
    },
}

#[derive(Subcommand)]
enum TodoAction {
    /// Add a new TODO item
    Add {
        /// Task description
        text: String,
        /// Scheduled time (ISO8601) — required
        #[arg(long)]
        at: String,
    },
    /// List all TODO items
    List,
    /// Mark a TODO item as done
    Done {
        /// Item ID
        id: u32,
    },
    /// Remove a TODO item
    Remove {
        /// Item ID
        id: u32,
    },
}

/// Send a request to the daemon and print the response. Bail on failure.
fn send(dir: &Path, req: &Request) -> Result<()> {
    let resp = socket::send_request(dir, req)?;
    if resp.ok {
        println!("{}", resp.message);
        Ok(())
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = cryochamber::work_dir()?;

    match cli.command {
        Commands::Hibernate {
            complete,
            exit,
            summary,
        } => send(
            &dir,
            &Request::Hibernate {
                complete,
                exit_code: exit,
                summary,
            },
        ),
        Commands::Note { text } => send(&dir, &Request::Note { text }),
        Commands::Send { text } | Commands::Reply { text } => send(&dir, &Request::Reply { text }),
        Commands::Alert {
            action,
            target,
            message,
        } => send(
            &dir,
            &Request::Alert {
                action,
                target,
                message,
            },
        ),
        Commands::Receive => cmd_receive(&dir),
        Commands::Time { offset, daily } => cmd_time(offset.as_deref(), daily.as_deref()),
        Commands::Todo { action } => cmd_todo(&dir, action),
    }
}

fn cmd_receive(dir: &Path) -> Result<()> {
    let messages = message::read_inbox(dir)?;
    if messages.is_empty() {
        println!("No messages.");
        return Ok(());
    }
    for (filename, msg) in &messages {
        println!("--- {} ---", filename);
        if !msg.from.is_empty() {
            println!("From: {}", msg.from);
        }
        if !msg.subject.is_empty() {
            println!("Subject: {}", msg.subject);
        }
        println!();
        println!("{}", msg.body);
        println!();
    }
    Ok(())
}

fn cmd_time(offset: Option<&str>, daily: Option<&str>) -> Result<()> {
    use chrono::Local;

    let now = Local::now();

    let target = if let Some(time_str) = daily {
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid daily time format. Use HH:MM (e.g. \"13:00\")");
        }
        let hour: u32 = parts[0]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid hour: {}", parts[0]))?;
        let minute: u32 = parts[1]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid minute: {}", parts[1]))?;

        let today = now
            .date_naive()
            .and_hms_opt(hour, minute, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid time: {}:{}", hour, minute))?;

        if now.naive_local() >= today {
            today + chrono::Duration::days(1)
        } else {
            today
        }
    } else {
        match offset {
            None => now.naive_local(),
            Some(s) => {
                let s = s.trim().trim_start_matches('+');
                let parts: Vec<&str> = s.splitn(2, ' ').collect();
                if parts.len() != 2 {
                    anyhow::bail!(
                        "Invalid offset format. Use e.g. \"+30 minutes\", \"+2 hours\", \"+1 day\""
                    );
                }
                let n: i64 = parts[0]
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid number: {}", parts[0]))?;
                let unit = parts[1].trim_end_matches('s');
                let duration = match unit {
                    "minute" | "min" => chrono::Duration::minutes(n),
                    "hour" | "hr" => chrono::Duration::hours(n),
                    "day" => chrono::Duration::days(n),
                    "week" => chrono::Duration::weeks(n),
                    _ => {
                        anyhow::bail!(
                            "Unknown time unit: {unit}. Use minutes, hours, days, or weeks."
                        )
                    }
                };
                (now + duration).naive_local()
            }
        }
    };

    println!("{}", target.format("%Y-%m-%dT%H:%M"));
    Ok(())
}

fn cmd_todo(dir: &Path, action: TodoAction) -> Result<()> {
    match action {
        TodoAction::Add { text, at } => send(dir, &Request::TodoAdd { text, at }),
        TodoAction::List => send(dir, &Request::TodoList),
        TodoAction::Done { id } => send(dir, &Request::TodoDone { id }),
        TodoAction::Remove { id } => send(dir, &Request::TodoRemove { id }),
    }
}
