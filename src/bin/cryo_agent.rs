// src/bin/cryo_agent.rs
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::Path;

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
    /// Reply to human (writes to outbox)
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
        Commands::Reply { text } => send(&dir, &Request::Reply { text }),
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
    }
}
