// src/bin/cryo_gh.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "cryo-gh", about = "Cryochamber GitHub Discussion sync")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize: create a Discussion and write gh-sync.json
    Init {
        /// GitHub repo in "owner/repo" format
        #[arg(long)]
        repo: String,
        /// Discussion title (default: derived from plan.md)
        #[arg(long)]
        title: Option<String>,
    },
    /// Pull new Discussion comments into messages/inbox/
    Pull,
    /// Push session summary to Discussion
    Push,
    /// Start background sync: pull Discussion comments → inbox, push outbox → Discussion
    Sync {
        /// Polling interval in seconds
        #[arg(long, default_value = "30")]
        interval: u64,
    },
    /// Stop the running sync daemon
    Unsync,
    /// Show sync status
    Status,
    /// Run the sync loop (internal — use `cryo-gh sync` instead)
    #[command(hide = true)]
    SyncDaemon {
        #[arg(long, default_value = "30")]
        interval: u64,
    },
}

fn gh_sync_path(dir: &Path) -> PathBuf {
    dir.join("gh-sync.json")
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { repo, title } => cmd_gh_init(&repo, title.as_deref()),
        Commands::Pull => cmd_gh_pull(),
        Commands::Push => cmd_gh_push(),
        Commands::Sync { interval } => cmd_gh_sync(interval),
        Commands::Unsync => cmd_gh_unsync(),
        Commands::Status => cmd_gh_status(),
        Commands::SyncDaemon { interval } => cmd_gh_sync_daemon(interval),
    }
}

fn cmd_gh_init(repo: &str, title: Option<&str>) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    let (owner, repo_name) = repo
        .split_once('/')
        .context("--repo must be in 'owner/repo' format")?;

    let default_title = format!(
        "[Cryo] {}",
        dir.file_name().unwrap_or_default().to_string_lossy()
    );
    let title = title.unwrap_or(&default_title);

    let plan_content = std::fs::read_to_string(dir.join("plan.md")).unwrap_or_default();
    let body = if plan_content.is_empty() {
        "Cryochamber sync Discussion.".to_string()
    } else {
        format!("## Cryochamber Plan\n\n{plan_content}")
    };

    println!("Creating GitHub Discussion in {repo}...");
    let (node_id, number) =
        cryochamber::channel::github::create_discussion(owner, repo_name, title, &body)?;
    println!("Created Discussion #{number}");

    let self_login = cryochamber::channel::github::whoami().ok();

    let sync_state = cryochamber::gh_sync::GhSyncState {
        repo: repo.to_string(),
        discussion_number: number,
        discussion_node_id: node_id,
        last_read_cursor: None,
        self_login,
        last_pushed_session: None,
    };
    cryochamber::gh_sync::save_sync_state(&gh_sync_path(&dir), &sync_state)?;
    println!("Saved gh-sync.json");

    Ok(())
}

fn cmd_gh_pull() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let mut sync_state = cryochamber::gh_sync::load_sync_state(&gh_sync_path(&dir))?
        .context("gh-sync.json not found. Run 'cryo-gh init' first.")?;

    let (owner, repo) = sync_state.owner_repo()?;

    println!(
        "Pulling comments from Discussion #{}...",
        sync_state.discussion_number
    );
    let new_cursor = cryochamber::channel::github::pull_comments(
        owner,
        repo,
        sync_state.discussion_number,
        sync_state.last_read_cursor.as_deref(),
        sync_state.self_login.as_deref(),
        &dir,
    )?;

    if let Some(cursor) = new_cursor {
        sync_state.last_read_cursor = Some(cursor);
        cryochamber::gh_sync::save_sync_state(&gh_sync_path(&dir), &sync_state)?;
    }

    let inbox = cryochamber::message::read_inbox(&dir)?;
    println!("Inbox: {} message(s)", inbox.len());

    Ok(())
}

fn cmd_gh_push() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let mut sync_state = cryochamber::gh_sync::load_sync_state(&gh_sync_path(&dir))?
        .context("gh-sync.json not found. Run 'cryo-gh init' first.")?;

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

    let comment = format!("## Session {session_num}\n\n```\n{session_output}\n```");

    println!(
        "Posting session summary to Discussion #{}...",
        sync_state.discussion_number
    );
    cryochamber::channel::github::post_comment(&sync_state.discussion_node_id, &comment)?;

    sync_state.last_pushed_session = Some(session_num);
    cryochamber::gh_sync::save_sync_state(&gh_sync_path(&dir), &sync_state)?;

    println!("Push complete.");
    Ok(())
}

fn cmd_gh_sync(interval: u64) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    // Require valid project (cryo.toml)
    if !cryochamber::config::config_path(&dir).exists() {
        anyhow::bail!("No cryochamber project in this directory. Run `cryo init` first.");
    }

    // Require initialized gh-sync.json
    let sync_path = gh_sync_path(&dir);
    let sync_state = cryochamber::gh_sync::load_sync_state(&sync_path)?
        .context("gh-sync.json not found. Run 'cryo-gh init' first.")?;

    // Ensure message dirs exist
    cryochamber::message::ensure_dirs(&dir)?;

    let exe = std::env::current_exe().context("Failed to resolve cryo-gh executable path")?;
    let interval_str = interval.to_string();
    let log_path = dir.join("cryo-gh-sync.log");
    cryochamber::service::install(
        "gh-sync",
        &dir,
        &exe,
        &["sync-daemon", "--interval", &interval_str],
        &log_path,
        true,
    )?;

    println!(
        "Sync service installed for Discussion #{} in {}",
        sync_state.discussion_number, sync_state.repo
    );
    println!("Log: cryo-gh-sync.log");
    println!("Survives reboot. Stop with: cryo-gh unsync");

    Ok(())
}

fn cmd_gh_unsync() -> Result<()> {
    let dir = cryochamber::work_dir()?;

    if cryochamber::service::uninstall("gh-sync", &dir)? {
        println!("Sync service stopped and removed.");
    } else {
        println!("No sync service installed for this directory.");
    }

    Ok(())
}

fn cmd_gh_sync_daemon(interval: u64) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let sync_path = gh_sync_path(&dir);

    eprintln!(
        "Sync daemon started (PID {})",
        std::process::id()
    );

    // Register signal handlers
    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))?;

    // Set up outbox watcher for immediate push on new messages
    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel();
    let outbox_path = dir.join("messages").join("outbox");
    let _watcher = {
        let tx = tx.clone();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
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

    // Spawn a thread to forward shutdown signals to the event channel
    let shutdown_flag = Arc::clone(&shutdown);
    std::thread::spawn(move || {
        while !shutdown_flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let _ = tx.send(()); // unblock recv_timeout
    });

    let interval_dur = std::time::Duration::from_secs(interval);

    loop {
        if shutdown.load(Ordering::Relaxed) {
            eprintln!("Sync: shutting down");
            break;
        }

        // Reload sync state each cycle (pull updates the cursor)
        let mut sync_state = cryochamber::gh_sync::load_sync_state(&sync_path)?
            .context("gh-sync.json disappeared")?;

        // Pull: Discussion → inbox
        let (owner, repo) = sync_state.owner_repo()?;
        match cryochamber::channel::github::pull_comments(
            owner,
            repo,
            sync_state.discussion_number,
            sync_state.last_read_cursor.as_deref(),
            sync_state.self_login.as_deref(),
            &dir,
        ) {
            Ok(new_cursor) => {
                if let Some(cursor) = new_cursor {
                    sync_state.last_read_cursor = Some(cursor);
                    cryochamber::gh_sync::save_sync_state(&sync_path, &sync_state)?;
                }
            }
            Err(e) => eprintln!("Sync: pull error: {e}"),
        }

        // Push: outbox → Discussion
        if let Err(e) = push_outbox(&dir, &sync_state) {
            eprintln!("Sync: push error: {e}");
        }

        // Wait for outbox event or interval timeout
        match rx.recv_timeout(interval_dur) {
            Ok(()) => {
                // Outbox changed or shutdown — small delay to let file writes complete
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    eprintln!("Sync: stopped");
    Ok(())
}

/// Read outbox messages and post each as a Discussion comment, then archive them.
fn push_outbox(
    dir: &Path,
    sync_state: &cryochamber::gh_sync::GhSyncState,
) -> Result<()> {
    let messages = cryochamber::message::read_outbox(dir)?;
    if messages.is_empty() {
        return Ok(());
    }

    let outbox = dir.join("messages").join("outbox");
    let archive = outbox.join("archive");
    std::fs::create_dir_all(&archive)?;

    for (filename, msg) in &messages {
        let body = format!("**{}** ({})\n\n{}", msg.from, msg.subject, msg.body);
        match cryochamber::channel::github::post_comment(&sync_state.discussion_node_id, &body) {
            Ok(()) => {
                eprintln!("Sync: posted outbox/{filename} to Discussion");
                let src = outbox.join(filename);
                let dst = archive.join(filename);
                if src.exists() {
                    std::fs::rename(&src, &dst)?;
                }
            }
            Err(e) => {
                eprintln!("Sync: failed to post outbox/{filename}: {e}");
            }
        }
    }

    Ok(())
}

fn cmd_gh_status() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    match cryochamber::gh_sync::load_sync_state(&gh_sync_path(&dir))? {
        None => println!("GitHub sync not configured. Run 'cryo-gh init' first."),
        Some(state) => {
            println!("Repo: {}", state.repo);
            println!("Discussion: #{}", state.discussion_number);
            println!(
                "Last read cursor: {}",
                state
                    .last_read_cursor
                    .as_deref()
                    .unwrap_or("(none — will read all)")
            );
        }
    }
    Ok(())
}
