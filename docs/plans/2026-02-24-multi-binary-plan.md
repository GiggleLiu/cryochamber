# Multi-Binary Split Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split the single `cryo` binary into three: `cryo` (operator), `cryo-agent` (agent IPC), `cryo-gh` (GitHub sync).

**Architecture:** Three `src/bin/*.rs` files sharing a single `src/lib.rs`. Shared helpers extracted from `main.rs` into library modules. `src/main.rs` deleted.

**Tech Stack:** Rust, clap (derive), existing cryochamber library crate.

---

### Task 1: Extract shared helpers into library

**Files:**
- Create: `src/process.rs`
- Modify: `src/lib.rs`
- Modify: `src/state.rs`
- Modify: `src/log.rs`

**Step 1: Create `src/process.rs` with process management utilities**

Move `send_signal`, `terminate_pid`, and `spawn_daemon` from `src/main.rs` into a new module. Note: `daemon.rs` has its own private `send_signal` — keep both (daemon's is internal to the session poll loop).

```rust
// src/process.rs
use anyhow::{Context, Result};
use std::path::Path;

/// Send a signal to a process, logging a warning on failure.
pub fn send_signal(pid: u32, signal: i32) {
    let ret = unsafe { libc::kill(pid as i32, signal) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        eprintln!("Warning: failed to send signal {signal} to PID {pid}: {err}");
    }
}

/// Send SIGTERM to a process, wait for it to exit, escalate to SIGKILL if needed.
pub fn terminate_pid(pid: u32) -> Result<()> {
    println!("Sending SIGTERM to process {pid}...");
    send_signal(pid, libc::SIGTERM);

    // Poll for up to 5 seconds
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let ret = unsafe { libc::kill(pid as i32, 0) };
        if ret != 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if errno != libc::EPERM {
                return Ok(()); // process is gone
            }
        }
    }

    // Escalate to SIGKILL
    println!("Process {pid} did not exit, sending SIGKILL...");
    send_signal(pid, libc::SIGKILL);
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}

/// Spawn the daemon subprocess in the background.
pub fn spawn_daemon(dir: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("cryo.log"))
        .context("Failed to open cryo.log")?;
    let err_file = log_file.try_clone().context("Failed to clone log handle")?;
    std::process::Command::new(&exe)
        .arg("daemon")
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(err_file)
        .spawn()
        .context("Failed to spawn daemon process")?;
    Ok(())
}
```

**Step 2: Add `work_dir`, `state_path`, `log_path` to library**

Add to `src/lib.rs` (top-level):

```rust
pub fn work_dir() -> anyhow::Result<std::path::PathBuf> {
    std::env::current_dir().context("Failed to get current directory")
}
```

Add to `src/state.rs`:

```rust
pub fn state_path(dir: &Path) -> PathBuf {
    dir.join("timer.json")
}
```

Add to `src/log.rs`:

```rust
pub fn log_path(dir: &Path) -> PathBuf {
    dir.join("cryo.log")
}
```

**Step 3: Register `process` module in `src/lib.rs`**

Add `pub mod process;` to `src/lib.rs`.

**Step 4: Run `cargo build` to verify library compiles**

Run: `cargo build`
Expected: compiles with warnings about unused imports (main.rs still has duplicates)

**Step 5: Commit**

```bash
git add src/process.rs src/lib.rs src/state.rs src/log.rs
git commit -m "feat: extract shared helpers into library modules"
```

---

### Task 2: Update Cargo.toml with three binary entries

**Files:**
- Modify: `Cargo.toml`

**Step 1: Replace the single `[[bin]]` block**

Change from:

```toml
[[bin]]
name = "cryo"
path = "src/main.rs"
```

To:

```toml
[[bin]]
name = "cryo"
path = "src/bin/cryo.rs"

[[bin]]
name = "cryo-agent"
path = "src/bin/cryo_agent.rs"

[[bin]]
name = "cryo-gh"
path = "src/bin/cryo_gh.rs"
```

**Step 2: Create empty bin files so cargo doesn't error**

Create `src/bin/cryo.rs`, `src/bin/cryo_agent.rs`, `src/bin/cryo_gh.rs` each with just:

```rust
fn main() {
    todo!()
}
```

**Step 3: Verify**

Run: `cargo build`
Expected: compiles (with dead code warnings for main.rs which is now orphaned)

**Step 4: Commit**

```bash
git add Cargo.toml src/bin/
git commit -m "chore: add three [[bin]] entries in Cargo.toml"
```

---

### Task 3: Create `src/bin/cryo_agent.rs`

**Files:**
- Modify: `src/bin/cryo_agent.rs`

**Step 1: Write the agent IPC binary**

```rust
// src/bin/cryo_agent.rs
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cryo-agent", about = "Cryochamber agent IPC commands")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// End session and schedule next wake
    Hibernate {
        /// Wake time in ISO8601 format
        #[arg(long)]
        wake: Option<String>,
        /// Mark plan as complete (no more wakes)
        #[arg(long)]
        complete: bool,
        /// Exit code: 0=success, 1=partial, 2=failure
        #[arg(long, default_value = "0")]
        exit: u8,
        /// Human-readable session summary
        #[arg(long)]
        summary: Option<String>,
    },
    /// Leave a note for the next session
    Note {
        /// Note text
        text: String,
    },
    /// Reply to human (writes to outbox)
    Reply {
        /// Reply message text
        text: String,
    },
    /// Set a fallback alert (dead-man switch)
    Alert {
        /// Action type (email, webhook)
        action: String,
        /// Target (email address, URL)
        target: String,
        /// Alert message
        message: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = cryochamber::work_dir()?;

    match cli.command {
        Commands::Hibernate {
            wake,
            complete,
            exit,
            summary,
        } => {
            if !complete && wake.is_none() {
                anyhow::bail!("Either --wake or --complete is required");
            }
            let req = cryochamber::socket::Request::Hibernate {
                wake,
                complete,
                exit_code: exit,
                summary,
            };
            let resp = cryochamber::socket::send_request(&dir, &req)?;
            if resp.ok {
                println!("{}", resp.message);
            } else {
                anyhow::bail!("{}", resp.message);
            }
        }
        Commands::Note { text } => {
            let req = cryochamber::socket::Request::Note { text };
            let resp = cryochamber::socket::send_request(&dir, &req)?;
            if resp.ok {
                println!("{}", resp.message);
            } else {
                anyhow::bail!("{}", resp.message);
            }
        }
        Commands::Reply { text } => {
            let req = cryochamber::socket::Request::Reply { text };
            let resp = cryochamber::socket::send_request(&dir, &req)?;
            if resp.ok {
                println!("{}", resp.message);
            } else {
                anyhow::bail!("{}", resp.message);
            }
        }
        Commands::Alert {
            action,
            target,
            message,
        } => {
            let req = cryochamber::socket::Request::Alert {
                action,
                target,
                message,
            };
            let resp = cryochamber::socket::send_request(&dir, &req)?;
            if resp.ok {
                println!("{}", resp.message);
            } else {
                anyhow::bail!("{}", resp.message);
            }
        }
    }
    Ok(())
}
```

**Step 2: Verify**

Run: `cargo build --bin cryo-agent`
Expected: compiles

**Step 3: Commit**

```bash
git add src/bin/cryo_agent.rs
git commit -m "feat: add cryo-agent binary for agent IPC"
```

---

### Task 4: Create `src/bin/cryo_gh.rs`

**Files:**
- Modify: `src/bin/cryo_gh.rs`

**Step 1: Write the GitHub sync binary**

Move all `cmd_gh_*` functions and the `GhCommands` enum from `src/main.rs`:

```rust
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
```

**Step 2: Verify**

Run: `cargo build --bin cryo-gh`
Expected: compiles

**Step 3: Commit**

```bash
git add src/bin/cryo_gh.rs
git commit -m "feat: add cryo-gh binary for GitHub sync"
```

---

### Task 5: Create `src/bin/cryo.rs` (operator binary)

**Files:**
- Modify: `src/bin/cryo.rs`

**Step 1: Write the operator binary**

This is the largest file — all of current `main.rs` minus agent IPC commands and gh commands. Uses the shared helpers from library.

```rust
// src/bin/cryo.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use cryochamber::message;
use cryochamber::protocol;
use cryochamber::session;
use cryochamber::state::{self, CryoState};

#[derive(Parser)]
#[command(name = "cryo", about = "Long-term AI agent task scheduler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a working directory with protocol file and template plan
    Init {
        /// Agent command to target (determines CLAUDE.md vs AGENTS.md)
        #[arg(long, default_value = "opencode")]
        agent: String,
    },
    /// Begin a new plan: initialize and run the first task
    Start {
        /// Path to plan file or directory containing plan.md (default: current directory)
        plan: Option<PathBuf>,
        /// Agent command to use (default: opencode)
        #[arg(long, default_value = "opencode")]
        agent: String,
        /// Max retry attempts on agent spawn failure (default: 1 = no retry)
        #[arg(long, default_value = "1")]
        max_retries: u32,
        /// Maximum session duration in seconds (0 = no timeout, default: no timeout)
        #[arg(long, default_value = "0")]
        max_session_duration: u64,
        /// Disable inbox file watching
        #[arg(long)]
        no_watch: bool,
    },
    /// Show current status: next wake time, last result
    Status,
    /// List all running cryo daemon processes on this machine
    Ps {
        /// Kill all listed daemons
        #[arg(long)]
        kill_all: bool,
    },
    /// Kill the running daemon and restart it
    Restart,
    /// Stop the daemon and remove state
    Cancel,
    /// Print the session log
    Log,
    /// Watch the session log in real-time
    Watch {
        /// Show full log from the beginning (default: start from current position)
        #[arg(long)]
        all: bool,
    },
    /// Send a message to the agent's inbox
    Send {
        /// Message body
        body: String,
        /// Sender name (default: "human")
        #[arg(long, default_value = "human")]
        from: String,
        /// Message subject (default: derived from body)
        #[arg(long)]
        subject: Option<String>,
    },
    /// Read messages from the agent's outbox
    Receive,
    /// Execute a fallback action (used internally by timers)
    FallbackExec {
        action: String,
        target: String,
        message: String,
    },
    /// Run the persistent daemon (internal — use `cryo start` instead)
    #[command(hide = true)]
    Daemon,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { agent } => cmd_init(&agent),
        Commands::Start {
            plan,
            agent,
            max_retries,
            max_session_duration,
            no_watch,
        } => cmd_start(
            plan.as_deref(),
            &agent,
            max_retries,
            max_session_duration,
            no_watch,
        ),
        Commands::Status => cmd_status(),
        Commands::Ps { kill_all } => cmd_ps(kill_all),
        Commands::Restart => cmd_restart(),
        Commands::Cancel => cmd_cancel(),
        Commands::Log => cmd_log(),
        Commands::Watch { all } => cmd_watch(all),
        Commands::Send {
            body,
            from,
            subject,
        } => cmd_send(&body, &from, subject.as_deref()),
        Commands::Daemon => cmd_daemon(),
        Commands::Receive => cmd_receive(),
        Commands::FallbackExec {
            action,
            target,
            message,
        } => {
            let dir = cryochamber::work_dir()?;
            let fb = cryochamber::fallback::FallbackAction {
                action,
                target,
                message,
            };
            fb.execute(&dir)
        }
    }
}

// Copy all cmd_* functions from main.rs verbatim, but replace:
//   work_dir()      -> cryochamber::work_dir()
//   state_path(&dir) -> cryochamber::state::state_path(&dir)
//   log_path(&dir)   -> cryochamber::log::log_path(&dir)
//   spawn_daemon     -> cryochamber::process::spawn_daemon
//   terminate_pid    -> cryochamber::process::terminate_pid
//
// Functions to copy:
//   cmd_init, validate_agent_command, cmd_start, cmd_daemon,
//   cmd_status, cmd_restart, cmd_ps, cmd_cancel, cmd_log,
//   cmd_send, cmd_receive, cmd_watch
//
// Do NOT copy: cmd_gh_*, gh_sync_path, GhCommands,
//   or the Hibernate/Note/Reply/Alert match arms
```

(The full function bodies are identical to current `main.rs` lines 335-690, with the helper substitutions noted above.)

**Step 2: Verify**

Run: `cargo build --bin cryo`
Expected: compiles

**Step 3: Commit**

```bash
git add src/bin/cryo.rs
git commit -m "feat: add cryo operator binary"
```

---

### Task 6: Delete `src/main.rs` and clean up daemon.rs

**Files:**
- Delete: `src/main.rs`
- Modify: `src/daemon.rs` (remove duplicate `send_signal`)

**Step 1: Delete main.rs**

```bash
rm src/main.rs
```

**Step 2: Replace daemon.rs `send_signal` with import from process module**

In `src/daemon.rs`, remove the local `send_signal` function (lines 24-31) and replace usages with `crate::process::send_signal`. There are two call sites in `run_one_session()`: lines 403 and 413-414.

**Step 3: Verify full build**

Run: `cargo build`
Expected: all three binaries compile cleanly

**Step 4: Commit**

```bash
git add -u
git commit -m "chore: delete main.rs, deduplicate send_signal"
```

---

### Task 7: Update protocol text to use `cryo-agent`

**Files:**
- Modify: `src/protocol.rs`

**Step 1: Replace `cryo` with `cryo-agent` in PROTOCOL_CONTENT**

Change these lines in the `PROTOCOL_CONTENT` const string:

```
cryo hibernate  ->  cryo-agent hibernate
cryo note       ->  cryo-agent note
cryo reply      ->  cryo-agent reply
cryo alert      ->  cryo-agent alert
cryo status     ->  cryo-agent status
cryo inbox      ->  cryo-agent inbox
```

Also update the Rules section:
```
Always call `cryo hibernate`  ->  Always call `cryo-agent hibernate`
Use `cryo note`               ->  Use `cryo-agent note`
Check `cryo inbox`            ->  Check `cryo-agent inbox`
Set `cryo alert`              ->  Set `cryo-agent alert`
```

**Step 2: Verify**

Run: `cargo build`
Expected: compiles

**Step 3: Commit**

```bash
git add src/protocol.rs
git commit -m "feat: update protocol text to reference cryo-agent"
```

---

### Task 8: Update tests

**Files:**
- Modify: `tests/cli_tests.rs`
- Modify: `tests/protocol_tests.rs`
- Modify: `tests/mock_agent.sh`

**Step 1: Update `tests/mock_agent.sh`**

The mock agent calls `$CRYO_BIN hibernate` and `$CRYO_BIN note`. After the split, these are `cryo-agent` commands. Change `CRYO_BIN` usage to `CRYO_AGENT_BIN`:

```sh
#!/bin/sh
# Mock agent for cryochamber integration tests.
# Uses cryo-agent CLI commands to communicate with the daemon.
# CRYO_AGENT_BIN must be set to the path of the cryo-agent binary.

echo "${MOCK_AGENT_OUTPUT:-Agent running}"

if [ -n "$MOCK_AGENT_NOTE" ]; then
    "$CRYO_AGENT_BIN" note "$MOCK_AGENT_NOTE" 2>/dev/null || true
fi

if [ "$MOCK_AGENT_COMPLETE" = "false" ] && [ -n "$MOCK_AGENT_WAKE" ]; then
    "$CRYO_AGENT_BIN" hibernate --wake "$MOCK_AGENT_WAKE" --summary "${MOCK_AGENT_SUMMARY:-continuing}" 2>/dev/null || true
else
    "$CRYO_AGENT_BIN" hibernate --complete --summary "${MOCK_AGENT_SUMMARY:-mock done}" 2>/dev/null || true
fi
```

**Step 2: Update `tests/cli_tests.rs`**

Update `cryo_bin_path()` to return the `cryo-agent` binary path, and rename to `cryo_agent_bin_path()`. Update all `.env("CRYO_BIN", ...)` to `.env("CRYO_AGENT_BIN", ...)`:

```rust
fn cryo_agent_bin_path() -> String {
    #[allow(deprecated)]
    let path = assert_cmd::cargo::cargo_bin("cryo-agent");
    path.to_string_lossy().to_string()
}
```

The `cmd()` helper stays as `cargo_bin("cryo")` — all operator tests still use the `cryo` binary.

Update the tests that use the mock agent (search for `CRYO_BIN`):
- `test_daemon_plan_complete` — change `.env("CRYO_BIN", cryo_bin_path())` to `.env("CRYO_AGENT_BIN", cryo_agent_bin_path())`
- `test_daemon_inbox_reactive_wake` — same
- `test_session_logs_inbox_filenames` — same

Add agent binary tests for `cryo-agent`:

```rust
fn agent_cmd() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cryo-agent").unwrap()
}

#[test]
fn test_agent_hibernate_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["hibernate", "--wake", "2099-01-01T00:00"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_agent_note_no_daemon() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["note", "test note"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot connect"));
}

#[test]
fn test_agent_hibernate_requires_wake_or_complete() {
    let dir = tempfile::tempdir().unwrap();
    agent_cmd()
        .args(["hibernate"])
        .current_dir(dir.path())
        .assert()
        .failure();
}
```

Remove the old `test_hibernate_no_daemon`, `test_note_no_daemon`, `test_reply_no_daemon`, `test_hibernate_complete_no_daemon`, `test_hibernate_requires_wake_or_complete` tests that tested these commands via the `cryo` binary (they no longer exist on `cryo`).

**Step 3: Update `tests/protocol_tests.rs`**

Update assertions to match new protocol text:

```rust
#[test]
fn test_protocol_content_contains_commands() {
    let content = protocol::PROTOCOL_CONTENT;
    assert!(content.contains("cryo-agent hibernate"));
    assert!(content.contains("cryo-agent note"));
    assert!(content.contains("cryo-agent reply"));
    assert!(content.contains("cryo-agent alert"));
}

#[test]
fn test_protocol_content_contains_rules() {
    let content = protocol::PROTOCOL_CONTENT;
    assert!(content.contains("Always call `cryo-agent hibernate`"));
    assert!(content.contains("plan.md"));
}

#[test]
fn test_protocol_mentions_hibernate() {
    let content = cryochamber::protocol::PROTOCOL_CONTENT;
    assert!(content.contains("cryo-agent hibernate"));
    assert!(content.contains("cryo-agent note"));
}
```

Also update `test_write_protocol_file` assertion:

```rust
assert!(content.contains("cryo-agent hibernate"));
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: all tests pass

**Step 5: Commit**

```bash
git add tests/
git commit -m "test: update tests for multi-binary split"
```

---

### Task 9: Update documentation

**Files:**
- Modify: `CLAUDE.md`
- Modify: `README.md`
- Modify: `cryo-skill.md`
- Modify: `examples/chess-by-mail/README.md` (if it references agent commands)

**Step 1: Update CLAUDE.md**

Update the architecture section to describe three binaries. Update any references to `cryo hibernate` etc. to `cryo-agent hibernate`.

**Step 2: Update README.md**

Search for all `cryo hibernate`, `cryo note`, `cryo reply`, `cryo alert` references and replace with `cryo-agent`. Add a note about the three-binary structure.

**Step 3: Update cryo-skill.md**

This is the agent-facing skill doc. All `cryo` agent commands become `cryo-agent`.

**Step 4: Check examples**

```bash
grep -r "cryo hibernate\|cryo note\|cryo reply\|cryo alert" examples/
```

Update any matches to `cryo-agent`.

**Step 5: Commit**

```bash
git add CLAUDE.md README.md cryo-skill.md examples/
git commit -m "docs: update documentation for multi-binary split"
```

---

### Task 10: Final verification

**Step 1: Full lint check**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings`
Expected: no errors

**Step 2: Full test suite**

Run: `cargo test`
Expected: all tests pass

**Step 3: Verify installation**

Run: `cargo install --path . --root /tmp/cryo-test`
Expected: installs `cryo`, `cryo-agent`, `cryo-gh` into `/tmp/cryo-test/bin/`

```bash
ls /tmp/cryo-test/bin/
```
Expected output includes: `cryo`, `cryo-agent`, `cryo-gh`

**Step 4: Smoke test each binary**

```bash
/tmp/cryo-test/bin/cryo --help
/tmp/cryo-test/bin/cryo-agent --help
/tmp/cryo-test/bin/cryo-gh --help
```

Expected: each prints its own help with correct command name and subcommands.

**Step 5: Commit (if any fmt/clippy fixes needed)**

```bash
git add -u
git commit -m "chore: final cleanup for multi-binary split"
```
