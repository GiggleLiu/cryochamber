// src/bin/cryo_agent.rs
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::io::Read;
use std::path::Path;

use cryochamber::socket::Request;
use cryochamber::todo::normalize_wake_input;

#[derive(Parser)]
#[command(name = "cryo-agent", about = "Cryochamber agent IPC commands", version)]
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
        #[arg(required_unless_present = "stdin", conflicts_with = "stdin")]
        text: Option<String>,
        /// Read message text from stdin. Use this for multi-line text or text
        /// containing shell-sensitive characters such as backticks or `$`.
        #[arg(long)]
        stdin: bool,
        /// Mark this message as a question awaiting a human reply.
        /// The hub rail shows a `?` badge until any human inbox message arrives.
        #[arg(long)]
        question: bool,
    },
    /// Read inbox messages from human
    Receive,
    /// Read the conversation transcript and claim any pending inbox batch
    Dialog(DialogArgs),
    /// Print current time, compute a future time, or validate an ISO8601 timestamp
    Time {
        /// Input: "+N minutes|hours|days|weeks" (relative offset) or an
        /// absolute ISO8601 timestamp like "2026-04-25T10:00" (also accepts
        /// :SS, a space separator, or date-only = midnight). Any other form
        /// (timezone offsets, natural language) is rejected.
        offset: Option<String>,
    },
    /// Manage TODO items across sessions
    Todo {
        #[command(subcommand)]
        action: TodoAction,
    },
}

#[derive(Args, Clone)]
struct DialogArgs {
    /// Show the last N messages (default: 20)
    #[arg(long)]
    last: Option<u32>,
    /// Show every archived message
    #[arg(long)]
    all: bool,
    /// Show messages with `at >= <iso-time>`
    #[arg(long)]
    since: Option<String>,
}

#[derive(Subcommand)]
enum TodoAction {
    /// Add a new TODO item
    Add {
        /// Task description
        text: String,
        /// Wake time. Accepted forms: "+30 minutes" (relative, units:
        /// minutes|hours|days|weeks), "2026-04-25T10:00" (ISO8601; also
        /// accepts :SS, a space separator, or date-only = midnight).
        /// Anything else is rejected with the list of accepted forms.
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
        Commands::Send {
            text,
            stdin,
            question,
        } => {
            let text = resolve_send_text(text, stdin)?;
            send(&dir, &Request::Send { text, question })
        }
        Commands::Receive => send(&dir, &Request::Receive),
        Commands::Dialog(args) => {
            let filter = dialog_filter_from_args(args)?;
            send(&dir, &Request::Dialog { filter })
        }
        Commands::Time { offset } => cmd_time(offset.as_deref()),
        Commands::Todo { action } => cmd_todo(&dir, action),
    }
}

fn resolve_send_text(text: Option<String>, stdin: bool) -> Result<String> {
    match (text, stdin) {
        (Some(text), false) => Ok(text),
        (None, true) => read_send_text_from_stdin(),
        (Some(_), true) => anyhow::bail!("provide either message text or --stdin, not both"),
        (None, false) => anyhow::bail!("missing message text; use --stdin to read from stdin"),
    }
}

fn read_send_text_from_stdin() -> Result<String> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text)
}

fn dialog_filter_from_args(args: DialogArgs) -> Result<cryochamber::socket::DialogFilter> {
    use cryochamber::socket::DialogFilter;

    let selected = args.last.is_some() as u8 + args.all as u8 + args.since.is_some() as u8;
    if selected > 1 {
        anyhow::bail!("--last, --all, and --since are mutually exclusive");
    }
    if args.all {
        return Ok(DialogFilter::All);
    }
    if let Some(iso) = args.since {
        return Ok(DialogFilter::Since { iso });
    }

    let count = args.last.unwrap_or(20);
    if count == 0 {
        anyhow::bail!("--last must be at least 1");
    }
    Ok(DialogFilter::LastN { count })
}

fn cmd_time(offset: Option<&str>) -> Result<()> {
    let now = chrono::Local::now().naive_local();
    let formatted = match offset {
        None => now.format("%Y-%m-%dT%H:%M").to_string(),
        Some(s) => normalize_wake_input(s, now)?,
    };
    println!("{formatted}");
    Ok(())
}

/// Resolve and validate a `--at` argument to the canonical wake-time form
/// before any IPC roundtrip, so a bad value fails loudly at the CLI.
fn resolve_at_arg(at: &str) -> Result<String> {
    normalize_wake_input(at, chrono::Local::now().naive_local())
        .map_err(|e| anyhow::anyhow!("invalid --at value: {e}"))
}

fn cmd_todo(dir: &Path, action: TodoAction) -> Result<()> {
    match action {
        TodoAction::Add { text, at } => {
            let at = resolve_at_arg(&at)?;
            send(dir, &Request::TodoAdd { text, at })
        }
        TodoAction::List => send(dir, &Request::TodoList),
        TodoAction::Done { id } => send(dir, &Request::TodoDone { id }),
        TodoAction::Remove { id } => send(dir, &Request::TodoRemove { id }),
    }
}

#[cfg(test)]
#[path = "unit_tests/cryo_agent.rs"]
mod time_tests;
