// src/bin/cryo_gh.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use cryochamber::channel::store::MessageStore;
use cryochamber::sync_common::{SyncLoopBackend, SyncLoopCommand};

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
        /// Polling interval in seconds (overrides cryo.toml gh_poll_interval)
        #[arg(long)]
        interval: Option<u64>,
    },
    /// Stop the running sync daemon
    Unsync,
    /// Show sync status
    Status,
    /// Run the sync loop (internal — use `cryo-gh sync` instead)
    #[command(hide = true)]
    SyncDaemon {
        #[arg(long)]
        interval: Option<u64>,
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

    let self_login = cryochamber::channel::github::whoami()
        .context("Failed to identify current GitHub user. Run `gh auth login` and retry.")?;

    let sync_state = cryochamber::gh_sync::GhSyncState {
        repo: repo.to_string(),
        discussion_number: number,
        discussion_node_id: node_id,
        last_read_cursor: None,
        self_login: Some(self_login),
        last_pushed_session: None,
    };
    cryochamber::gh_sync::save_sync_state(&gh_sync_path(&dir), &sync_state)?;
    println!("Saved gh-sync.json");

    Ok(())
}

fn load_gh_sync_state_with_self_login(
    sync_path: &Path,
) -> Result<cryochamber::gh_sync::GhSyncState> {
    let mut sync_state = cryochamber::gh_sync::load_sync_state(sync_path)?
        .context("gh-sync.json not found. Run 'cryo-gh init' first.")?;

    if sync_state.ensure_self_login_with(|| {
        cryochamber::channel::github::whoami()
            .context("Failed to identify current GitHub user. Run `gh auth login` and retry.")
    })? {
        cryochamber::gh_sync::save_sync_state(sync_path, &sync_state)?;
    }

    Ok(sync_state)
}

fn cmd_gh_pull() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let sync_path = gh_sync_path(&dir);
    let mut sync_state = load_gh_sync_state_with_self_login(&sync_path)?;

    println!(
        "Pulling comments from Discussion #{}...",
        sync_state.discussion_number
    );
    if pull_discussion_comments_into_inbox(&dir, &mut sync_state)? {
        cryochamber::gh_sync::save_sync_state(&sync_path, &sync_state)?;
    }

    let inbox = MessageStore::new(dir).read_inbox_named()?;
    println!("Inbox: {} message(s)", inbox.len());

    Ok(())
}

fn cmd_gh_push() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let sync_path = gh_sync_path(&dir);
    let mut sync_state = load_gh_sync_state_with_self_login(&sync_path)?;

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
    cryochamber::gh_sync::save_sync_state(&sync_path, &sync_state)?;

    println!("Push complete.");
    Ok(())
}

fn resolve_interval(interval_override: Option<u64>) -> Result<u64> {
    let dir = cryochamber::work_dir()?;
    let cfg = cryochamber::config::load_config(&cryochamber::config::config_path(&dir))?
        .unwrap_or_default();
    Ok(interval_override.unwrap_or(cfg.gh_poll_interval))
}

fn cmd_gh_sync(interval_override: Option<u64>) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    // Require valid project (cryo.toml)
    if !cryochamber::config::config_path(&dir).exists() {
        anyhow::bail!("No cryochamber project in this directory. Run `cryo init` first.");
    }

    let interval = resolve_interval(interval_override)?;

    // Require initialized gh-sync.json
    let sync_path = gh_sync_path(&dir);
    let sync_state = load_gh_sync_state_with_self_login(&sync_path)?;

    // Ensure message dirs exist
    MessageStore::new(dir.clone()).ensure_dirs()?;

    let exe = std::env::current_exe().context("Failed to resolve cryo-gh executable path")?;
    let interval_str = interval.to_string();
    let log_path = dir.join("cryo-gh-sync.log");
    cryochamber::service::install(
        "gh-sync",
        &dir,
        &exe,
        &["sync-daemon", "--interval", &interval_str],
        &log_path,
        false,
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

struct GhSyncLoopBackend {
    dir: PathBuf,
    sync_path: PathBuf,
    sync_state: Option<cryochamber::gh_sync::GhSyncState>,
}

impl SyncLoopBackend for GhSyncLoopBackend {
    fn receive(&mut self) -> Result<SyncLoopCommand> {
        // Reload sync state each cycle (pull updates the cursor).
        // Config-level errors (missing login, unreadable state file, missing
        // repo) are unrecoverable without operator intervention; surface them
        // as Halt so the loop exits cleanly with a visible reason instead of
        // restart-looping against the same wall.
        let mut sync_state = match load_gh_sync_state_with_self_login(&self.sync_path) {
            Ok(state) => state,
            Err(e) => {
                return Ok(SyncLoopCommand::Halt {
                    reason: format!("gh sync config error: {e:#}"),
                });
            }
        };

        // Pull: Discussion -> inbox
        let (owner, repo) = match sync_state.owner_repo() {
            Ok(pair) => pair,
            Err(e) => {
                return Ok(SyncLoopCommand::Halt {
                    reason: format!("gh sync repo not configured: {e:#}"),
                });
            }
        };
        let owner = owner.to_string();
        let repo = repo.to_string();

        match pull_discussion_comments_into_inbox_from(&self.dir, &mut sync_state, &owner, &repo) {
            Ok(cursor_changed) => {
                if cursor_changed {
                    cryochamber::gh_sync::save_sync_state(&self.sync_path, &sync_state)?;
                }
            }
            Err(e) => match cryochamber::sync_common::classify_sync_error(&e) {
                cryochamber::sync_common::SyncErrorKind::AuthOrConfig => {
                    return Ok(SyncLoopCommand::Halt {
                        reason: format!("gh sync pull auth/config error: {e:#}"),
                    });
                }
                cryochamber::sync_common::SyncErrorKind::Transient => {
                    eprintln!("Sync: pull error (transient): {e}");
                }
            },
        }

        self.sync_state = Some(sync_state);
        Ok(SyncLoopCommand::Send)
    }

    fn send(&mut self) -> Result<cryochamber::sync_common::SyncCycleStatus> {
        let Some(sync_state) = self.sync_state.as_ref() else {
            return Ok(cryochamber::sync_common::SyncCycleStatus::Continue);
        };

        // Push: outbox -> Discussion
        if let Err(e) = push_outbox(&self.dir, sync_state) {
            match cryochamber::sync_common::classify_sync_error(&e) {
                cryochamber::sync_common::SyncErrorKind::AuthOrConfig => {
                    return Ok(cryochamber::sync_common::SyncCycleStatus::Halt {
                        reason: format!("gh sync push auth/config error: {e:#}"),
                    });
                }
                cryochamber::sync_common::SyncErrorKind::Transient => {
                    eprintln!("Sync: push error (transient): {e}");
                }
            }
        }

        Ok(cryochamber::sync_common::SyncCycleStatus::Continue)
    }
}

fn pull_discussion_comments_into_inbox(
    dir: &Path,
    sync_state: &mut cryochamber::gh_sync::GhSyncState,
) -> Result<bool> {
    let (owner, repo) = sync_state.owner_repo()?;
    let owner = owner.to_string();
    let repo = repo.to_string();
    pull_discussion_comments_into_inbox_from(dir, sync_state, &owner, &repo)
}

fn pull_discussion_comments_into_inbox_from(
    dir: &Path,
    sync_state: &mut cryochamber::gh_sync::GhSyncState,
    owner: &str,
    repo: &str,
) -> Result<bool> {
    let store = MessageStore::new(dir.to_path_buf());
    let result = cryochamber::channel::github::fetch_comments(
        owner,
        repo,
        sync_state.discussion_number,
        sync_state.last_read_cursor.as_deref(),
        sync_state.self_login.as_deref(),
    )?;

    for msg in &result.messages {
        store.send_in(msg)?;
    }

    if let Some(cursor) = result.cursor {
        sync_state.last_read_cursor = Some(cursor);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn cmd_gh_sync_daemon(interval_override: Option<u64>) -> Result<()> {
    let interval = resolve_interval(interval_override)?;
    let dir = cryochamber::work_dir()?;
    let sync_path = gh_sync_path(&dir);

    eprintln!("Sync daemon started (PID {})", std::process::id());
    let pid_path = cryochamber::gh_sync::sync_pid_path(&dir);
    // RAII guard: keeps the pid file on disk for the daemon's lifetime and
    // unlinks it on any return, including early `?` propagation below.
    let _pid_guard = cryochamber::sync_common::PidFile::create(pid_path)?;

    // Register signal handlers
    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))?;

    // Set up outbox watcher for immediate push on new messages
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = cryochamber::sync_common::watch_outbox(&dir, tx.clone())?;

    // Spawn a thread to forward shutdown signals to the event channel
    cryochamber::sync_common::spawn_shutdown_notifier(Arc::clone(&shutdown), tx);

    let interval_dur = std::time::Duration::from_secs(interval);
    let mut backend = GhSyncLoopBackend {
        dir: dir.clone(),
        sync_path,
        sync_state: None,
    };

    cryochamber::sync_common::run_sync_loop(
        "Sync",
        Arc::clone(&shutdown),
        rx,
        interval_dur,
        &mut backend,
    )?;
    // _pid_guard drops here, unlinking the pid file.
    Ok(())
}

/// Read outbox messages and post each as a Discussion comment, then archive them.
fn push_outbox(dir: &Path, sync_state: &cryochamber::gh_sync::GhSyncState) -> Result<()> {
    cryochamber::sync_common::push_outbox_messages(
        dir,
        |filename| format!("Sync: posted outbox/{filename} to Discussion"),
        |body| cryochamber::channel::github::post_comment(&sync_state.discussion_node_id, body),
    )
}

fn cmd_gh_status() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    match cryochamber::gh_sync::load_sync_state(&gh_sync_path(&dir))? {
        None => println!("GitHub sync not configured. Run 'cryo-gh init' first."),
        Some(state) => {
            for line in state.status_lines() {
                println!("{line}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "unit_tests/cryo_gh.rs"]
mod tests;
