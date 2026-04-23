// src/bin/cryo_zulip.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use cryochamber::channel::store::MessageStore;
use cryochamber::channel::zulip::ZulipClient;
use cryochamber::sync_common::{format_outbox_post, SyncLoopBackend, SyncLoopCommand};

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
        /// Import existing stream/topic history instead of only future messages
        #[arg(long)]
        history: bool,
    },
    /// Pull new messages from Zulip stream into messages/inbox/
    Pull,
    /// Push session summary to Zulip stream
    Push,
    /// Start background sync daemon
    Sync {
        /// Polling interval in seconds (overrides cryo.toml zulip_poll_interval)
        #[arg(long)]
        interval: Option<u64>,
    },
    /// Stop the running sync daemon
    Unsync,
    /// Show sync status
    Status,
    /// Run the sync loop (internal — use `cryo-zulip sync` instead)
    #[command(hide = true)]
    SyncDaemon {
        #[arg(long)]
        interval: Option<u64>,
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
            history,
        } => cmd_init(&config, &stream, topic.as_deref(), history),
        Commands::Pull => cmd_pull(),
        Commands::Push => cmd_push(),
        Commands::Sync { interval } => cmd_sync(interval),
        Commands::Unsync => cmd_unsync(),
        Commands::Status => cmd_status(),
        Commands::SyncDaemon { interval } => cmd_sync_daemon(interval),
    }
}

fn cmd_init(
    config_path: &str,
    stream_name: &str,
    topic: Option<&str>,
    history: bool,
) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    let client = ZulipClient::from_zuliprc(Path::new(config_path))?;

    println!("Validating credentials...");
    let (_user_id, self_email) = client.get_profile()?;
    println!("Authenticated as {self_email}");

    println!("Resolving stream '{stream_name}'...");
    let stream_id = client.get_stream_id(stream_name).map_err(|e| {
        anyhow::anyhow!(
            "Could not resolve stream '{stream_name}'. Likely causes:\n  \
             1. The stream does not exist — verify the name in Zulip.\n  \
             2. The bot ({self_email}) is not subscribed to the stream —\n     \
                add it in Zulip: stream settings → Subscribers → add user.\n  \
             3. The stream is private and the bot lacks access.\n\n\
             Underlying error: {e}"
        )
    })?;
    println!("Stream ID: {stream_id}");

    let topic_name = topic.unwrap_or("cryochamber");
    let newest_message_id = if history {
        None
    } else {
        client.newest_message_id(stream_id, Some(topic_name))?
    };
    let last_message_id =
        cryochamber::zulip_sync::initial_last_message_id(history, newest_message_id);

    let sync_state = cryochamber::zulip_sync::ZulipSyncState {
        site: client.credentials().site.clone(),
        stream: stream_name.to_string(),
        stream_id,
        self_email,
        topic: topic.map(|t| t.to_string()),
        last_message_id,
        last_pushed_session: None,
    };
    cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;

    copy_zuliprc_to_project(Path::new(config_path), &dir)?;

    println!("Saved zulip-sync.json");
    println!("Copied zuliprc to .cryo/zuliprc");
    println!("{}", init_import_message(history, last_message_id));
    Ok(())
}

fn init_import_message(history: bool, last_message_id: Option<u64>) -> String {
    match (history, last_message_id) {
        (true, _) => "Existing messages will be imported on first pull.".to_string(),
        (false, Some(id)) => {
            format!("Only messages newer than Zulip message {id} will be imported.")
        }
        (false, None) => {
            "No existing messages found; future messages will be imported.".to_string()
        }
    }
}

fn copy_zuliprc_to_project(config_path: &Path, dir: &Path) -> Result<()> {
    let cryo_dir = dir.join(".cryo");
    std::fs::create_dir_all(&cryo_dir)?;
    let dest = cryo_dir.join("zuliprc");

    let same_file = match (
        std::fs::canonicalize(config_path),
        std::fs::canonicalize(&dest),
    ) {
        (Ok(src), Ok(dst)) => src == dst,
        _ => false,
    };
    if same_file {
        return Ok(());
    }

    std::fs::copy(config_path, &dest)
        .with_context(|| format!("Failed to copy zuliprc to {}", dest.display()))?;
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
    if pull_zulip_messages_into_inbox(&dir, &client, &mut sync_state)? {
        cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;
    }

    let inbox = MessageStore::new(dir).read_inbox_named()?;
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

fn resolve_interval(interval_override: Option<u64>) -> Result<u64> {
    let dir = cryochamber::work_dir()?;
    let cfg = cryochamber::config::load_config(&cryochamber::config::config_path(&dir))?
        .unwrap_or_default();
    Ok(interval_override.unwrap_or(cfg.zulip_poll_interval))
}

fn cmd_sync(interval_override: Option<u64>) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    if !cryochamber::config::config_path(&dir).exists() {
        anyhow::bail!("No cryochamber project in this directory. Run `cryo init` first.");
    }

    let interval = resolve_interval(interval_override)?;

    let sync_path = zulip_sync_path(&dir);
    let sync_state = cryochamber::zulip_sync::load_sync_state(&sync_path)?
        .context("zulip-sync.json not found. Run 'cryo-zulip init' first.")?;

    MessageStore::new(dir.clone()).ensure_dirs()?;

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

struct ZulipSyncLoopBackend {
    dir: PathBuf,
    sync_path: PathBuf,
    cycle_state: Option<(ZulipClient, cryochamber::zulip_sync::ZulipSyncState)>,
}

impl SyncLoopBackend for ZulipSyncLoopBackend {
    fn receive(&mut self) -> Result<SyncLoopCommand> {
        self.cycle_state = None;
        // Config-level errors (missing zuliprc, stream not found, bad
        // credentials) are unrecoverable without operator intervention;
        // surface them as Halt so the loop exits cleanly with a visible
        // reason instead of skipping sends forever.
        let (client, mut sync_state) = match load_client_from_project(&self.dir) {
            Ok(pair) => pair,
            Err(e) => {
                return Ok(SyncLoopCommand::Halt {
                    reason: format!("zulip sync config error: {e:#}"),
                });
            }
        };

        // Pull: Zulip -> inbox
        match pull_zulip_messages_into_inbox(&self.dir, &client, &mut sync_state) {
            Ok(cursor_changed) => {
                if cursor_changed {
                    if let Err(e) =
                        cryochamber::zulip_sync::save_sync_state(&self.sync_path, &sync_state)
                    {
                        eprintln!("Zulip sync: failed to save state: {e}");
                    }
                }
            }
            Err(e) => match cryochamber::sync_common::classify_sync_error(&e) {
                cryochamber::sync_common::SyncErrorKind::AuthOrConfig => {
                    return Ok(SyncLoopCommand::Halt {
                        reason: format!("zulip sync pull auth/config error: {e:#}"),
                    });
                }
                cryochamber::sync_common::SyncErrorKind::Transient => {
                    eprintln!("Zulip sync: pull error (transient): {e}");
                }
            },
        }

        self.cycle_state = Some((client, sync_state));
        Ok(SyncLoopCommand::Send)
    }

    fn send(&mut self) -> Result<cryochamber::sync_common::SyncCycleStatus> {
        let Some((client, sync_state)) = self.cycle_state.take() else {
            return Ok(cryochamber::sync_common::SyncCycleStatus::Continue);
        };

        // Push: outbox -> Zulip
        if let Err(e) = push_outbox(&self.dir, &client, &sync_state) {
            match cryochamber::sync_common::classify_sync_error(&e) {
                cryochamber::sync_common::SyncErrorKind::AuthOrConfig => {
                    return Ok(cryochamber::sync_common::SyncCycleStatus::Halt {
                        reason: format!("zulip sync push auth/config error: {e:#}"),
                    });
                }
                cryochamber::sync_common::SyncErrorKind::Transient => {
                    eprintln!("Zulip sync: push error (transient): {e}");
                }
            }
        }

        Ok(cryochamber::sync_common::SyncCycleStatus::Continue)
    }
}

fn pull_zulip_messages_into_inbox(
    dir: &Path,
    client: &ZulipClient,
    sync_state: &mut cryochamber::zulip_sync::ZulipSyncState,
) -> Result<bool> {
    let store = MessageStore::new(dir.to_path_buf());
    let result = client.fetch_messages_since(
        sync_state.stream_id,
        Some(sync_state.topic_name()),
        sync_state.last_message_id,
        Some(&sync_state.self_email),
    )?;

    for msg in &result.messages {
        store.send_in(msg)?;
    }

    let new_last_id = cryochamber::zulip_sync::remember_seen_message_id(
        sync_state.last_message_id,
        result.newest_seen_id,
    );
    if let Some(id) = new_last_id {
        if sync_state.last_message_id != Some(id) {
            sync_state.last_message_id = Some(id);
            return Ok(true);
        }
    }
    Ok(false)
}

fn cmd_sync_daemon(interval_override: Option<u64>) -> Result<()> {
    let interval = resolve_interval(interval_override)?;
    let dir = cryochamber::work_dir()?;
    let sync_path = zulip_sync_path(&dir);

    eprintln!("Zulip sync daemon started (PID {})", std::process::id());
    let pid_path = cryochamber::zulip_sync::sync_pid_path(&dir);
    // RAII guard: unlinks the pid file on any return, including early `?`
    // propagation from signal_hook / notify setup or per-cycle save_sync_state
    // failures below. Without this, a stale pid file lingers and a recycled
    // PID can make the hub report the daemon as running forever.
    let _pid_guard = cryochamber::sync_common::PidFile::create(pid_path)?;

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))?;

    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = cryochamber::sync_common::watch_outbox(&dir, tx.clone())?;
    cryochamber::sync_common::spawn_shutdown_notifier(Arc::clone(&shutdown), tx);

    let interval_dur = std::time::Duration::from_secs(interval);
    let mut backend = ZulipSyncLoopBackend {
        dir: dir.clone(),
        sync_path,
        cycle_state: None,
    };

    cryochamber::sync_common::run_sync_loop(
        "Zulip sync",
        Arc::clone(&shutdown),
        rx,
        interval_dur,
        &mut backend,
    )?;
    // _pid_guard drops here, unlinking the pid file.
    Ok(())
}

fn push_outbox(
    dir: &Path,
    client: &ZulipClient,
    sync_state: &cryochamber::zulip_sync::ZulipSyncState,
) -> Result<()> {
    let store = MessageStore::new(dir.to_path_buf());
    let messages = store.read_outbox_named()?;
    if messages.is_empty() {
        return Ok(());
    }

    let topic = sync_state.topic_name();

    for (filename, msg) in &messages {
        let body = format_outbox_post(msg);
        match client.send_message(sync_state.stream_id, topic, &body) {
            Ok(_) => {
                eprintln!("Zulip sync: posted outbox/{filename}");
                store.archive_outbox(std::slice::from_ref(filename))?;
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

#[cfg(test)]
#[path = "unit_tests/cryo_zulip.rs"]
mod format_tests;
