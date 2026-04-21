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
        Some(sync_state.topic_name()),
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
            Some(sync_state.topic_name()),
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
    // _pid_guard drops here, unlinking the pid file.
    Ok(())
}

/// Format an outbox message for posting to a Zulip stream.
///
/// Zulip already shows the sender's bot name above each message, so we don't
/// re-state who wrote it in the body.
///
/// - Agent replies (`from == "agent"`): post the body as-is. The subject is
///   always "Reply", which adds no information.
/// - System messages (`from == "cryochamber"`: reports, fallback alerts):
///   render as a Zulip blockquote with the subject as a bold header. The
///   blockquote visually marks them as machine-generated rather than a
///   human-style reply.
/// - Anything else: keep the original `**from** (subject)\n\nbody` shape so
///   non-system, non-agent senders remain attributable.
fn format_outbox_post(msg: &cryochamber::message::Message) -> String {
    if msg.from == "agent" {
        msg.body.clone()
    } else if msg.from == "cryochamber" {
        let mut out = format!("> **{}**\n>\n", msg.subject);
        for line in msg.body.lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
        // Trim the trailing newline that the loop added.
        if out.ends_with('\n') {
            out.pop();
        }
        out
    } else {
        format!("**{}** ({})\n\n{}", msg.from, msg.subject, msg.body)
    }
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
        let body = format_outbox_post(msg);
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

#[cfg(test)]
mod format_tests {
    use super::*;
    use chrono::NaiveDateTime;
    use cryochamber::message::Message;
    use std::collections::BTreeMap;

    fn mk(from: &str, subject: &str, body: &str) -> Message {
        Message {
            from: from.into(),
            subject: subject.into(),
            body: body.into(),
            timestamp: NaiveDateTime::default(),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn agent_reply_posts_body_only() {
        // Zulip already shows the bot name above the message — re-stating
        // "**agent**" in the body just adds noise. The subject is always
        // "Reply" anyway, which is information-free.
        let out = format_outbox_post(&mk("agent", "Reply", "hello human"));
        assert_eq!(out, "hello human");
    }

    #[test]
    fn cryochamber_report_renders_as_blockquote() {
        // Reports are machine-generated; render them as a Zulip blockquote
        // so they read as system info rather than a human-style reply.
        let out = format_outbox_post(&mk(
            "cryochamber",
            "Cryochamber Report: demo",
            "Last 24h: 3 sessions, 0 failed",
        ));
        assert_eq!(
            out,
            "> **Cryochamber Report: demo**\n>\n> Last 24h: 3 sessions, 0 failed"
        );
    }

    #[test]
    fn cryochamber_multiline_body_quotes_each_line() {
        let out = format_outbox_post(&mk(
            "cryochamber",
            "Fallback Alert: deadline_missed",
            "Agent exceeded max retries.\nNext attempt in 60s.",
        ));
        assert_eq!(
            out,
            "> **Fallback Alert: deadline_missed**\n>\n> Agent exceeded max retries.\n> Next attempt in 60s."
        );
    }

    #[test]
    fn unknown_sender_keeps_attribution() {
        // Anything that isn't agent/cryochamber should still identify itself.
        let out = format_outbox_post(&mk("teammate", "Question", "Are you free?"));
        assert_eq!(out, "**teammate** (Question)\n\nAre you free?");
    }
}
