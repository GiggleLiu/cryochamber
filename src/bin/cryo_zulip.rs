// src/bin/cryo_zulip.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cryochamber::channel::zulip::ZulipClient;

#[derive(Parser)]
#[command(name = "cryo-zulip", about = "Cryochamber Zulip sync")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize: validate credentials, resolve stream, write zulip-sync.json
    Init {
        /// Path to zuliprc file
        #[arg(long)]
        config: String,
        /// Zulip stream name
        #[arg(long)]
        stream: String,
        /// Topic name for outgoing messages (default: "cryochamber")
        #[arg(long)]
        topic: Option<String>,
    },
    /// Pull new messages from Zulip stream into messages/inbox/
    Pull,
    /// Push session summary to Zulip stream
    Push,
    /// Start background sync daemon
    Sync {
        /// Polling interval in seconds
        #[arg(long, default_value = "30")]
        interval: u64,
    },
    /// Stop the running sync daemon
    Unsync,
    /// Show sync status
    Status,
    /// Run the sync loop (internal — use `cryo-zulip sync` instead)
    #[command(hide = true)]
    SyncDaemon {
        #[arg(long, default_value = "30")]
        interval: u64,
    },
}

fn zulip_sync_path(dir: &Path) -> PathBuf {
    dir.join("zulip-sync.json")
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            config,
            stream,
            topic,
        } => cmd_init(&config, &stream, topic.as_deref()),
        Commands::Pull => cmd_pull(),
        Commands::Push => cmd_push(),
        Commands::Sync { interval } => cmd_sync(interval),
        Commands::Unsync => cmd_unsync(),
        Commands::Status => cmd_status(),
        Commands::SyncDaemon { interval } => cmd_sync_daemon(interval),
    }
}

fn cmd_init(config_path: &str, stream_name: &str, topic: Option<&str>) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    let client = ZulipClient::from_zuliprc(Path::new(config_path))?;

    println!("Validating credentials...");
    let (_user_id, self_email) = client.get_profile()?;
    println!("Authenticated as {self_email}");

    println!("Resolving stream '{stream_name}'...");
    let stream_id = client.get_stream_id(stream_name)?;
    println!("Stream ID: {stream_id}");

    let sync_state = cryochamber::zulip_sync::ZulipSyncState {
        site: client.credentials().site.clone(),
        stream: stream_name.to_string(),
        stream_id,
        self_email,
        topic: topic.map(|t| t.to_string()),
        last_message_id: None,
        last_pushed_session: None,
    };
    cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;

    // Copy zuliprc to .cryo/ for later use by pull/push/sync
    let cryo_dir = dir.join(".cryo");
    std::fs::create_dir_all(&cryo_dir)?;
    std::fs::copy(config_path, cryo_dir.join("zuliprc"))?;

    println!("Saved zulip-sync.json");
    println!("Copied zuliprc to .cryo/zuliprc");
    Ok(())
}

fn load_client_from_project(
    dir: &Path,
) -> Result<(ZulipClient, cryochamber::zulip_sync::ZulipSyncState)> {
    let sync_state = cryochamber::zulip_sync::load_sync_state(&zulip_sync_path(dir))?
        .context("zulip-sync.json not found. Run 'cryo-zulip init' first.")?;
    let rc_path = dir.join(".cryo").join("zuliprc");
    let client = ZulipClient::from_zuliprc(&rc_path)
        .context("Failed to load .cryo/zuliprc. Re-run 'cryo-zulip init'.")?;
    Ok((client, sync_state))
}

fn cmd_pull() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let (client, mut sync_state) = load_client_from_project(&dir)?;

    println!("Pulling messages from stream '{}'...", sync_state.stream);
    let new_last_id = client.pull_messages(
        sync_state.stream_id,
        sync_state.last_message_id,
        Some(&sync_state.self_email),
        &dir,
    )?;

    if let Some(id) = new_last_id {
        if sync_state.last_message_id != Some(id) {
            sync_state.last_message_id = Some(id);
            cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;
        }
    }

    let inbox = cryochamber::message::read_inbox(&dir)?;
    println!("Inbox: {} message(s)", inbox.len());
    Ok(())
}

fn cmd_push() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let (client, mut sync_state) = load_client_from_project(&dir)?;

    let log = cryochamber::log::log_path(&dir);
    let latest = cryochamber::log::read_latest_session(&log)?;

    let Some(session_output) = latest else {
        println!("No session log found. Nothing to push.");
        return Ok(());
    };

    let state_file = cryochamber::state::state_path(&dir);
    let session_num = cryochamber::state::load_state(&state_file)?
        .map(|s| s.session_number)
        .unwrap_or(0);

    if sync_state.last_pushed_session == Some(session_num) {
        println!("Session {session_num} already pushed. Skipping.");
        return Ok(());
    }

    let topic = sync_state.topic_name();
    let comment = format!("## Session {session_num}\n\n```\n{session_output}\n```");

    println!(
        "Posting session summary to stream '{}'...",
        sync_state.stream
    );
    client.send_message(sync_state.stream_id, topic, &comment)?;

    sync_state.last_pushed_session = Some(session_num);
    cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;

    println!("Push complete.");
    Ok(())
}

fn cmd_sync(interval: u64) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    if !cryochamber::config::config_path(&dir).exists() {
        anyhow::bail!("No cryochamber project in this directory. Run `cryo init` first.");
    }

    let sync_path = zulip_sync_path(&dir);
    let sync_state = cryochamber::zulip_sync::load_sync_state(&sync_path)?
        .context("zulip-sync.json not found. Run 'cryo-zulip init' first.")?;

    cryochamber::message::ensure_dirs(&dir)?;

    let exe = std::env::current_exe().context("Failed to resolve cryo-zulip executable path")?;
    let interval_str = interval.to_string();
    let log_path = dir.join("cryo-zulip-sync.log");
    cryochamber::service::install(
        "zulip-sync",
        &dir,
        &exe,
        &["sync-daemon", "--interval", &interval_str],
        &log_path,
        true,
    )?;

    println!(
        "Sync service installed for stream '{}' on {}",
        sync_state.stream, sync_state.site
    );
    println!("Log: cryo-zulip-sync.log");
    println!("Survives reboot. Stop with: cryo-zulip unsync");
    Ok(())
}

fn cmd_unsync() -> Result<()> {
    let dir = cryochamber::work_dir()?;

    if cryochamber::service::uninstall("zulip-sync", &dir)? {
        println!("Sync service stopped and removed.");
    } else {
        println!("No sync service installed for this directory.");
    }
    Ok(())
}

fn cmd_sync_daemon(interval: u64) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let sync_path = zulip_sync_path(&dir);

    eprintln!("Zulip sync daemon started (PID {})", std::process::id());

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))?;

    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel();
    let outbox_path = dir.join("messages").join("outbox");
    let _watcher = {
        let tx = tx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event.kind.is_create() {
                    let _ = tx.send(());
                }
            }
        })
        .context("Failed to create outbox watcher")?;
        watcher
            .watch(&outbox_path, notify::RecursiveMode::NonRecursive)
            .context("Failed to watch messages/outbox/")?;
        watcher
    };

    let shutdown_flag = Arc::clone(&shutdown);
    std::thread::spawn(move || {
        while !shutdown_flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let _ = tx.send(());
    });

    let interval_dur = std::time::Duration::from_secs(interval);

    loop {
        if shutdown.load(Ordering::Relaxed) {
            eprintln!("Zulip sync: shutting down");
            break;
        }

        let (client, mut sync_state) = match load_client_from_project(&dir) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("Zulip sync: config error: {e}");
                std::thread::sleep(interval_dur);
                continue;
            }
        };

        // Pull: Zulip → inbox
        match client.pull_messages(
            sync_state.stream_id,
            sync_state.last_message_id,
            Some(&sync_state.self_email),
            &dir,
        ) {
            Ok(new_last_id) => {
                if let Some(id) = new_last_id {
                    if sync_state.last_message_id != Some(id) {
                        sync_state.last_message_id = Some(id);
                        if let Err(e) =
                            cryochamber::zulip_sync::save_sync_state(&sync_path, &sync_state)
                        {
                            eprintln!("Zulip sync: failed to save state: {e}");
                        }
                    }
                }
            }
            Err(e) => eprintln!("Zulip sync: pull error: {e}"),
        }

        // Push: outbox → Zulip
        if let Err(e) = push_outbox(&dir, &client, &sync_state) {
            eprintln!("Zulip sync: push error: {e}");
        }

        match rx.recv_timeout(interval_dur) {
            Ok(()) => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    eprintln!("Zulip sync: stopped");
    Ok(())
}

fn push_outbox(
    dir: &Path,
    client: &ZulipClient,
    sync_state: &cryochamber::zulip_sync::ZulipSyncState,
) -> Result<()> {
    let messages = cryochamber::message::read_outbox(dir)?;
    if messages.is_empty() {
        return Ok(());
    }

    let outbox = dir.join("messages").join("outbox");
    let archive = outbox.join("archive");
    std::fs::create_dir_all(&archive)?;

    let topic = sync_state.topic_name();

    for (filename, msg) in &messages {
        let body = format!("**{}** ({})\n\n{}", msg.from, msg.subject, msg.body);
        match client.send_message(sync_state.stream_id, topic, &body) {
            Ok(_) => {
                eprintln!("Zulip sync: posted outbox/{filename}");
                let src = outbox.join(filename);
                let dst = archive.join(filename);
                if src.exists() {
                    std::fs::rename(&src, &dst)?;
                }
            }
            Err(e) => {
                eprintln!("Zulip sync: failed to post outbox/{filename}: {e}");
            }
        }
    }

    Ok(())
}

fn cmd_status() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    match cryochamber::zulip_sync::load_sync_state(&zulip_sync_path(&dir))? {
        None => println!("Zulip sync not configured. Run 'cryo-zulip init' first."),
        Some(state) => {
            println!("Site: {}", state.site);
            println!("Stream: {} (ID: {})", state.stream, state.stream_id);
            println!("Topic: {}", state.topic_name());
            println!("Bot email: {}", state.self_email);
            println!(
                "Last message ID: {}",
                state
                    .last_message_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "(none — will read all)".to_string())
            );
            println!(
                "Last pushed session: {}",
                state
                    .last_pushed_session
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "(none)".to_string())
            );
        }
    }
    Ok(())
}
