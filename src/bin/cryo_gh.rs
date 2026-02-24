// src/bin/cryo_gh.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

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
    /// Pull then push (full sync)
    Sync,
    /// Show sync status
    Status,
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
        Commands::Sync => {
            cmd_gh_pull()?;
            cmd_gh_push()
        }
        Commands::Status => cmd_gh_status(),
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
