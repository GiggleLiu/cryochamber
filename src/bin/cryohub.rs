// src/bin/cryohub.rs
//! Cryohub — workspace-wide web dashboard for managing cryochambers.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

const SERVICE_LABEL: &str = "hub";
const LOG_FILENAME: &str = "cryohub.log";
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8765;

#[derive(Parser)]
#[command(
    name = "cryohub",
    about = "Cryochamber hub: workspace-wide web dashboard"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the hub (installs an OS service that survives reboot unless --foreground)
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
    /// Stop and remove the hub service
    Stop,
    /// Show whether a hub service is installed for this workspace
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

fn require_workspace() -> Result<std::path::PathBuf> {
    let dir = cryochamber::work_dir()?;
    if dir.join("chambers").is_dir() {
        return Ok(dir);
    }
    if cryochamber::config::config_path(&dir).exists() {
        anyhow::bail!(
            "cryohub runs in workspace mode.\n\n\
             This directory contains a cryo.toml (it's a chamber), not a chambers/ directory.\n\
             Create a workspace:\n  \
               mkdir -p ~/cryo-workspace/chambers\n  \
               ln -s {} ~/cryo-workspace/chambers/{}\n  \
               cd ~/cryo-workspace && cryohub start\n",
            dir.display(),
            dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("this-chamber"),
        );
    }
    anyhow::bail!(
        "cryohub needs a workspace: a directory containing a `chambers/` subdirectory.\n\
         {} has no `chambers/` here.\n\
         Create one with:\n  \
           mkdir -p {dir}/chambers\n\
         or symlink an existing chamber into it.",
        dir.display(),
        dir = dir.display(),
    );
}

fn cmd_start(host: Option<String>, port: Option<u16>, foreground: bool) -> Result<()> {
    let dir = require_workspace()?;
    let host = host.unwrap_or_else(|| DEFAULT_HOST.to_string());
    let port = port.unwrap_or(DEFAULT_PORT);

    if foreground {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(cryochamber::hub::serve(dir, &host, port));
    }

    let exe = std::env::current_exe().context("Failed to resolve cryohub executable path")?;
    let port_str = port.to_string();
    let log_path = dir.join(LOG_FILENAME);
    cryochamber::service::install(
        SERVICE_LABEL,
        &dir,
        &exe,
        &["daemon", "--host", &host, "--port", &port_str],
        &log_path,
        true,
    )?;
    println!("Cryohub service installed: http://{host}:{port}");
    println!("Log: {}", log_path.display());
    println!("Survives reboot. Stop with: cryohub stop");
    Ok(())
}

fn cmd_stop() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    if cryochamber::service::uninstall(SERVICE_LABEL, &dir)? {
        println!("Cryohub service stopped and removed.");
    } else {
        println!("No cryohub service installed for this directory.");
    }
    Ok(())
}

fn cmd_status() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    if cryochamber::service::is_installed(SERVICE_LABEL, &dir) {
        println!(
            "Cryohub service: installed ({})",
            cryochamber::service::service_label(SERVICE_LABEL, &dir)
        );
        let log = dir.join(LOG_FILENAME);
        if log.exists() {
            println!("Log: {}", log.display());
        }
    } else {
        println!("Cryohub service: not installed for {}", dir.display());
    }
    Ok(())
}

fn cmd_daemon(host: String, port: u16) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cryochamber::hub::serve(dir, &host, port))
}
