// src/bin/cryohub.rs
//! Cryohub — directory-scoped web dashboard for managing cryochambers.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

const SERVICE_LABEL: &str = "hub";
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8765;

#[derive(Parser)]
#[command(
    name = "cryohub",
    about = "Cryochamber hub: directory-scoped web dashboard"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the hub from the current directory (installs an OS service that
    /// survives reboot unless --foreground)
    Start {
        /// Host to listen on (default: 127.0.0.1)
        #[arg(long)]
        host: Option<String>,
        /// Port to listen on (default: 8765)
        #[arg(long)]
        port: Option<u16>,
        /// Run in foreground instead of installing a service
        #[arg(long)]
        foreground: bool,
    },
    /// Stop and remove the hub service for the current directory
    Stop,
    /// Show whether a hub service is installed for the current directory.
    /// Always also lists every other cryohub service installed on the machine.
    Status,
    /// Run the server in the current process (internal — used by the service)
    #[command(hide = true)]
    Daemon {
        #[arg(long)]
        host: String,
        #[arg(long)]
        port: u16,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start {
            host,
            port,
            foreground,
        } => cmd_start(host, port, foreground),
        Commands::Stop => cmd_stop(),
        Commands::Status => cmd_status(),
        Commands::Daemon { host, port } => cmd_daemon(host, port),
    }
}

fn require_chambers_dir() -> Result<PathBuf> {
    let dir = cryochamber::work_dir()?;
    if cryochamber::config::config_path(&dir).exists() {
        anyhow::bail!(
            "{} is a chamber (contains cryo.toml), not a directory of chambers.\n\
             cd to the parent directory that holds your chamber subdirectories\n\
             and run cryohub from there.",
            dir.display()
        );
    }
    Ok(dir)
}

fn cmd_start(host: Option<String>, port: Option<u16>, foreground: bool) -> Result<()> {
    let dir = require_chambers_dir()?;
    let host = host.unwrap_or_else(|| DEFAULT_HOST.to_string());
    let port = port.unwrap_or(DEFAULT_PORT);

    if foreground {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(cryochamber::hub::serve(dir, &host, port));
    }

    let exe = std::env::current_exe().context("Failed to resolve cryohub executable path")?;
    let port_str = port.to_string();
    let log_path = cryochamber::hub::paths::hub_log_path(&dir);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    cryochamber::service::install(
        SERVICE_LABEL,
        &dir,
        &exe,
        &["daemon", "--host", &host, "--port", &port_str],
        &log_path,
        true,
    )?;
    println!("Cryohub service installed: http://{host}:{port}");
    println!("Serving chambers from: {}", dir.display());
    println!("Log: {}", log_path.display());
    println!(
        "Survives reboot. Stop with: cd {} && cryohub stop",
        dir.display()
    );
    Ok(())
}

fn cmd_stop() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    if cryochamber::service::uninstall(SERVICE_LABEL, &dir)? {
        println!("Cryohub service stopped and removed.");
        return Ok(());
    }
    println!("No cryohub service installed for {}.", dir.display());
    print_other_installed(&dir);
    Ok(())
}

fn cmd_status() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    if cryochamber::service::is_installed(SERVICE_LABEL, &dir) {
        println!(
            "Cryohub service: installed ({})",
            cryochamber::service::service_label(SERVICE_LABEL, &dir)
        );
        let log = cryochamber::hub::paths::hub_log_path(&dir);
        if log.exists() {
            println!("Log: {}", log.display());
        }
    } else {
        println!("Cryohub service: not installed for {}", dir.display());
    }
    print_other_installed(&dir);
    Ok(())
}

/// List every cryohub service on the machine *except* the one anchored at
/// `dir` (which the caller has already reported on). Helps users find services
/// they started from a different cwd.
fn print_other_installed(dir: &Path) {
    let installed = cryochamber::service::list_installed(SERVICE_LABEL);
    let others: Vec<_> = installed
        .into_iter()
        .filter(|s| s.dir.as_deref() != Some(dir))
        .collect();
    if others.is_empty() {
        return;
    }
    println!("\nOther cryohub services installed on this machine:");
    for s in others {
        match s.dir {
            Some(d) => println!("  {} → {}", s.label, d.display()),
            None => println!("  {} → (working directory not parseable)", s.label),
        }
    }
    println!("(cd into the listed directory and run `cryohub stop` to remove one.)");
}

fn cmd_daemon(host: String, port: u16) -> Result<()> {
    let dir = require_chambers_dir()?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cryochamber::hub::serve(dir, &host, port))
}
