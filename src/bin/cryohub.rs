// src/bin/cryohub.rs
//! Cryohub — global web dashboard for managing cryochambers.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

const SERVICE_LABEL: &str = "hub";

#[derive(Parser)]
#[command(name = "cryohub", about = "Cryochamber hub: global web dashboard")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the global hub (installs an OS service that survives reboot unless --foreground)
    Start {
        /// Host to listen on (overrides cryohub.toml)
        #[arg(long)]
        host: Option<String>,
        /// Port to listen on (overrides cryohub.toml)
        #[arg(long)]
        port: Option<u16>,
        /// Run in foreground instead of installing a service
        #[arg(long)]
        foreground: bool,
    },
    /// Stop and remove the global hub service
    Stop,
    /// Show whether the global hub service is installed.
    Status,
    /// Run the server in the current process (internal - used by the service)
    #[command(hide = true)]
    Daemon {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
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

fn cmd_start(host: Option<String>, port: Option<u16>, foreground: bool) -> Result<()> {
    let config = cryochamber::hub::config::effective_config(host, port)?;
    std::fs::create_dir_all(&config.chamber_root)?;

    if foreground {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(cryochamber::hub::serve(&config.host, config.port));
    }

    let exe = std::env::current_exe().context("Failed to resolve cryohub executable path")?;
    let service_dir = cryochamber::hub::paths::hub_service_dir();
    std::fs::create_dir_all(&service_dir)?;
    let args = ["daemon"];
    let log_path = cryochamber::hub::paths::hub_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    cryochamber::service::install(SERVICE_LABEL, &service_dir, &exe, &args, &log_path, true)?;
    let actual_log = cryochamber::service::stdio_log_path(SERVICE_LABEL, &service_dir, &log_path);
    println!(
        "Cryohub service installed: http://{}:{}",
        config.host, config.port
    );
    println!("Chamber root: {}", config.chamber_root.display());
    println!(
        "Config: {}",
        cryochamber::hub::paths::hub_config_path().display()
    );
    println!("Log: {}", actual_log.display());
    println!("Survives reboot. Stop with: cryohub stop");
    Ok(())
}

fn cmd_stop() -> Result<()> {
    let service_dir = cryochamber::hub::paths::hub_service_dir();
    if cryochamber::service::uninstall(SERVICE_LABEL, &service_dir)? {
        println!("Cryohub service stopped and removed.");
        return Ok(());
    }
    println!("No global cryohub service installed.");
    print_legacy_installed();
    Ok(())
}

fn cmd_status() -> Result<()> {
    let service_dir = cryochamber::hub::paths::hub_service_dir();
    let config = cryochamber::hub::config::load_config()?;
    if cryochamber::service::is_installed(SERVICE_LABEL, &service_dir) {
        println!(
            "Cryohub service: installed ({})",
            cryochamber::service::service_label(SERVICE_LABEL, &service_dir)
        );
        let fallback = cryochamber::hub::paths::hub_log_path();
        let log = cryochamber::service::stdio_log_path(SERVICE_LABEL, &service_dir, &fallback);
        if log.exists() {
            println!("Log: {}", log.display());
        }
    } else {
        println!("Cryohub service: not installed");
    }
    println!("URL: http://{}:{}", config.host, config.port);
    println!("Chamber root: {}", config.chamber_root.display());
    println!(
        "Config: {}",
        cryochamber::hub::paths::hub_config_path().display()
    );
    print_legacy_installed();
    Ok(())
}

/// List older cwd-scoped hub services, if any, so users can clean them up
/// after upgrading to the single global hub service.
fn print_legacy_installed() {
    let service_dir = cryochamber::hub::paths::hub_service_dir();
    let installed = cryochamber::service::list_installed(SERVICE_LABEL);
    let others: Vec<_> = installed
        .into_iter()
        .filter(|s| s.dir.as_deref() != Some(service_dir.as_path()))
        .collect();
    if others.is_empty() {
        return;
    }
    println!("\nLegacy cwd-scoped cryohub services installed on this machine:");
    for s in others {
        match s.dir {
            Some(d) => println!("  {} → {}", s.label, d.display()),
            None => println!("  {} → (working directory not parseable)", s.label),
        }
    }
    println!("(These are from older Cryohub versions; remove them from their listed directories.)");
}

fn cmd_daemon(host: Option<String>, port: Option<u16>) -> Result<()> {
    let config = cryochamber::hub::config::effective_config(host, port)?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cryochamber::hub::serve(&config.host, config.port))
}
