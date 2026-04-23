// src/bin/cryo_agent.rs
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::Path;

use cryochamber::{message, socket::Request};

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
        /// Exit code: 0=success, nonzero=failure (daemon retries)
        #[arg(long, default_value = "0")]
        exit: u8,
        /// Human-readable session summary
        #[arg(long)]
        summary: Option<String>,
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
    /// Read inbox messages from human
    Receive,
    /// Print current time, compute a future time, or validate an ISO8601 timestamp
    Time {
        /// Input: "+N minutes|hours|days|weeks" (relative offset)
        /// or an absolute ISO8601 timestamp like "2026-04-25T10:00"
        offset: Option<String>,
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
    let resp = cryochamber::daemon_client::send_checked_request(dir, req)?;
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
        Commands::Send { text } | Commands::Reply { text } => send(&dir, &Request::Reply { text }),
        Commands::Receive => cmd_receive(&dir),
        Commands::Time { offset } => cmd_time(offset.as_deref()),
        Commands::Todo { action } => cmd_todo(&dir, action),
    }
}

fn cmd_receive(dir: &Path) -> Result<()> {
    let messages = message::read_inbox(dir)?;
    if messages.is_empty() {
        println!("No messages in inbox.");
        return Ok(());
    }

    let filenames: Vec<String> = messages.iter().map(|(name, _)| name.clone()).collect();
    println!("{}", message::format_inbox(&messages));
    message::archive_messages(dir, &filenames)?;
    Ok(())
}

fn cmd_time(offset: Option<&str>) -> Result<()> {
    use chrono::Local;

    let now = Local::now();

    let formatted = match offset {
        None => now.format("%Y-%m-%dT%H:%M").to_string(),
        Some(s) => {
            let s = s.trim();
            if looks_like_iso_date(s) {
                parse_iso_timestamp(s)?
            } else {
                let dt = now + parse_relative_offset(s)?;
                dt.format("%Y-%m-%dT%H:%M").to_string()
            }
        }
    };

    println!("{formatted}");
    Ok(())
}

/// Accepted forms for `cryo-agent time` input, as a user-facing error body.
fn time_usage_error(got: &str) -> String {
    format!(
        "unrecognized time expression {got:?}.\n\
         Accepted forms:\n  \
           (no argument)          # current time\n  \
           +30 minutes            # relative offset (minutes|hours|days|weeks)\n  \
           2026-04-25T10:00       # absolute ISO8601\n\
         For natural expressions like \"tomorrow 9am\", compute the absolute\n\
         timestamp yourself from the current time and pass it directly."
    )
}

/// Heuristic: input starts with `YYYY-MM-DD` → try ISO8601.
fn looks_like_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

/// Parse an ISO8601-ish absolute timestamp and return it normalized to `%Y-%m-%dT%H:%M`.
fn parse_iso_timestamp(s: &str) -> Result<String> {
    let dt_formats = [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
    ];
    for fmt in &dt_formats {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt.format("%Y-%m-%dT%H:%M").to_string());
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date.and_hms_opt(0, 0, 0).unwrap();
        return Ok(dt.format("%Y-%m-%dT%H:%M").to_string());
    }
    anyhow::bail!("{}", time_usage_error(s))
}

/// Parse "+N minutes|hours|days|weeks" (the `+` is optional).
fn parse_relative_offset(s: &str) -> Result<chrono::Duration> {
    let rel = s.trim_start_matches('+');
    let parts: Vec<&str> = rel.splitn(2, ' ').collect();
    if parts.len() != 2 {
        anyhow::bail!("{}", time_usage_error(s));
    }
    let n: i64 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("{}", time_usage_error(s)))?;
    let unit = parts[1].trim_end_matches('s');
    match unit {
        "minute" | "min" => Ok(chrono::Duration::minutes(n)),
        "hour" | "hr" => Ok(chrono::Duration::hours(n)),
        "day" => Ok(chrono::Duration::days(n)),
        "week" => Ok(chrono::Duration::weeks(n)),
        _ => anyhow::bail!("{}", time_usage_error(s)),
    }
}

fn cmd_todo(dir: &Path, action: TodoAction) -> Result<()> {
    match action {
        TodoAction::Add { text, at } => send(dir, &Request::TodoAdd { text, at }),
        TodoAction::List => send(dir, &Request::TodoList),
        TodoAction::Done { id } => send(dir, &Request::TodoDone { id }),
        TodoAction::Remove { id } => send(dir, &Request::TodoRemove { id }),
    }
}

#[cfg(test)]
#[path = "unit_tests/cryo_agent.rs"]
mod time_tests;
