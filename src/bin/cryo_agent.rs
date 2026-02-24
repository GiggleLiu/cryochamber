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
        /// Wake time in ISO8601 format
        #[arg(long)]
        wake: Option<String>,
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
            wake,
            complete,
            exit,
            summary,
        } => {
            if !complete && wake.is_none() {
                anyhow::bail!("Either --wake or --complete is required");
            }
            send(
                &dir,
                &Request::Hibernate {
                    wake,
                    complete,
                    exit_code: exit,
                    summary,
                },
            )
        }
        Commands::Note { text } => send(&dir, &Request::Note { text }),
        Commands::Send { text } | Commands::Reply { text } => {
            send(&dir, &Request::Reply { text })
        }
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
        Commands::Time { offset } => cmd_time(offset.as_deref()),
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

fn cmd_time(offset: Option<&str>) -> Result<()> {
    use chrono::Local;

    let now = Local::now();

    let target = match offset {
        None => now,
        Some(s) => {
            let s = s.trim().trim_start_matches('+');
            let parts: Vec<&str> = s.splitn(2, ' ').collect();
            if parts.len() != 2 {
                anyhow::bail!("Invalid offset format. Use e.g. \"+30 minutes\", \"+2 hours\", \"+1 day\"");
            }
            let n: i64 = parts[0]
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid number: {}", parts[0]))?;
            let unit = parts[1].trim_end_matches('s'); // "minutes" -> "minute"
            let duration = match unit {
                "minute" | "min" => chrono::Duration::minutes(n),
                "hour" | "hr" => chrono::Duration::hours(n),
                "day" => chrono::Duration::days(n),
                "week" => chrono::Duration::weeks(n),
                _ => anyhow::bail!("Unknown time unit: {unit}. Use minutes, hours, days, or weeks."),
            };
            now + duration
        }
    };

    println!("{}", target.format("%Y-%m-%dT%H:%M"));
    Ok(())
}
