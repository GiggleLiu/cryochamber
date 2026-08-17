// src/bin/cryohub.rs
//! Cryohub — global web dashboard for managing cryochambers.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

const SERVICE_LABEL: &str = "hub";

#[derive(Parser)]
#[command(
    name = "cryohub",
    about = "Cryochamber hub: global web dashboard",
    version
)]
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
        /// Enforce bearer auth (the default). Saved to cryohub.toml.
        #[arg(long)]
        public: bool,
        /// Run without authentication (open mode, loopback only). Sharing and
        /// invites do not work in open mode. Saved to cryohub.toml, so
        /// disabling auth is never implicit: a plain `cryohub start` keeps
        /// whatever mode is saved.
        #[arg(long, conflicts_with = "public")]
        no_public: bool,
    },
    /// Stop and remove the global hub service
    Stop,
    /// Restart the global hub service without reinstalling it
    Restart,
    /// Show whether the global hub service is installed.
    Status,
    /// Manage access tokens for --public mode
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
    /// Run the server in the current process (internal - used by the service)
    #[command(hide = true)]
    Daemon {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        public: bool,
    },
}

#[derive(Subcommand)]
enum TokenAction {
    /// Create (if absent) and print the owner token
    Owner,
    /// Create a named invite scoped to chamber ids
    Create {
        #[arg(long)]
        name: String,
        /// Comma-separated chamber ids
        #[arg(long, value_delimiter = ',')]
        chambers: Vec<String>,
    },
    /// List invites (never prints token strings)
    List,
    /// Revoke an invite by name
    Revoke { name: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start {
            host,
            port,
            foreground,
            public,
            no_public,
        } => cmd_start(host, port, foreground, public_override(public, no_public)),
        Commands::Stop => cmd_stop(),
        Commands::Restart => cmd_restart(),
        Commands::Status => cmd_status(),
        Commands::Token { action } => cmd_token(action),
        Commands::Daemon { host, port, public } => cmd_daemon(host, port, public.then_some(true)),
    }
}

/// Turn the two mode flags into an override for the saved config. Absent both,
/// `None` leaves the saved mode alone — public mode must survive a plain
/// restart, and turning it off must be deliberate.
fn public_override(public: bool, no_public: bool) -> Option<bool> {
    match (public, no_public) {
        (true, _) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
    }
}

fn cmd_start(
    host: Option<String>,
    port: Option<u16>,
    foreground: bool,
    public: Option<bool>,
) -> Result<()> {
    let config = cryochamber::hub::config::effective_config(host, port, public)?;
    config.validate_console_dir()?;
    std::fs::create_dir_all(&config.chamber_root)?;

    // Before binding a socket AND before installing a service: a public hub
    // needs an owner token to be administrable at all, and the operator has to
    // see it while they are still at the terminal — a service start would
    // otherwise print it into a log file nobody reads.
    if config.public {
        if let Some(token) = cryochamber::hub::ensure_owner_token()? {
            cryochamber::hub::announce_owner_token(&token);
        }
    }

    if foreground {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(cryochamber::hub::serve(
            &config.host,
            config.port,
            config.public,
        ));
    }

    let exe = std::env::current_exe().context("Failed to resolve cryohub executable path")?;
    let service_dir = cryochamber::hub::paths::hub_service_dir();
    std::fs::create_dir_all(&service_dir)?;
    // The installed unit re-invokes this binary. The mode is persisted in
    // cryohub.toml, but pass it explicitly too so the unit is self-describing.
    let args: &[&str] = if config.public {
        &["daemon", "--public"]
    } else {
        &["daemon"]
    };
    let log_path = cryochamber::hub::paths::hub_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    cryochamber::service::install(SERVICE_LABEL, &service_dir, &exe, args, &log_path, true)?;
    let actual_log = cryochamber::service::stdio_log_path(SERVICE_LABEL, &service_dir, &log_path);
    println!(
        "Cryohub service installed: http://{}:{}",
        config.host, config.port
    );
    if config.public {
        println!("Mode: PUBLIC (bearer auth enforced on every /api route)");
    }
    println!("Chamber root: {}", config.chamber_root.display());
    println!("Console: {}", config.console_source().describe());
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

fn cmd_restart() -> Result<()> {
    let service_dir = cryochamber::hub::paths::hub_service_dir();
    if cryochamber::service::restart(SERVICE_LABEL, &service_dir)? {
        println!("Cryohub service restarted.");
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
    println!(
        "Mode: {}",
        if config.public {
            "public (bearer auth)"
        } else {
            "open (loopback)"
        }
    );
    println!("Chamber root: {}", config.chamber_root.display());
    println!("Console: {}", config.console_source().describe());
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

/// The service-unit entry point. Reads the config and honours the unit's flags
/// in memory only — a boot is not a configuration act, and re-saving here is
/// how an older binary once dropped a key it did not know.
fn cmd_daemon(host: Option<String>, port: Option<u16>, public: Option<bool>) -> Result<()> {
    let config = cryochamber::hub::config::overlay_config(
        cryochamber::hub::config::load_config()?,
        host,
        port,
        public,
    );
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cryochamber::hub::serve(
        &config.host,
        config.port,
        config.public,
    ))
}

/// Unwrap a token transaction. Both failure modes abort the command; only the
/// wording differs, and a persistence failure additionally guarantees that
/// nothing was written, so the operator can safely retry.
fn unwrap_mutation<T>(result: Result<T, cryochamber::hub::auth::MutateError>) -> Result<T> {
    use cryochamber::hub::auth::MutateError;
    match result {
        Ok(value) => Ok(value),
        Err(MutateError::Rejected(e)) => Err(e),
        Err(MutateError::Persist(e)) => {
            Err(e.context("token store could not be written; no change was made"))
        }
    }
}

fn cmd_token(action: TokenAction) -> Result<()> {
    use cryochamber::hub::{auth::AuthCtx, tokens};

    // Go through `AuthCtx` rather than load/mutate/save by hand, so the CLI
    // gets the same transaction as the API: the change is persisted before it
    // is considered to have happened.
    let path = tokens::default_tokens_path();
    let ctx = AuthCtx::load(&path)?;
    match action {
        TokenAction::Owner => {
            let token = unwrap_mutation(ctx.mutate(|store| store.ensure_owner()))?;
            // Bare on stdout so `cryohub token owner` composes in a pipeline;
            // everything explanatory goes to stderr.
            println!("{token}");
            eprintln!("Owner token stored in {}", path.display());
        }
        TokenAction::Create { name, chambers } => {
            let invite = unwrap_mutation(ctx.mutate(|store| store.create_invite(&name, chambers)))?;
            // The only moment this secret is ever printed — it is not
            // recoverable from `token list` or the API afterwards.
            println!("token: {}", invite.token);
            println!("link fragment: #invite={}", invite.token);
        }
        TokenAction::List => {
            ctx.with_store(|store| {
                if store.invites.is_empty() {
                    println!("(no invites)");
                    return;
                }
                println!("NAME\tSTATUS\tCHAMBERS\tCREATED\tREVOKED");
                for i in &store.invites {
                    let status = if i.revoked_at.is_some() {
                        "revoked"
                    } else {
                        "active"
                    };
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        i.name,
                        status,
                        i.chambers.join(","),
                        i.created_at,
                        i.revoked_at.as_deref().unwrap_or("-")
                    );
                }
            });
        }
        TokenAction::Revoke { name } => {
            unwrap_mutation(ctx.mutate(|store| {
                anyhow::ensure!(store.revoke(&name), "no active invite named '{name}'");
                Ok(())
            }))?;
            println!("revoked {name}");
        }
    }
    Ok(())
}
