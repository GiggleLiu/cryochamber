//! Shared sync backend abstraction: summary types and lifecycle wrappers.
//! Two backends (gh, zulip) with near-identical verbs -- free functions are
//! enough; no trait needed.

use crate::message::Message;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncBackend {
    Gh,
    Zulip,
}

impl SyncBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncBackend::Gh => "gh",
            SyncBackend::Zulip => "zulip",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gh" => Some(SyncBackend::Gh),
            "zulip" => Some(SyncBackend::Zulip),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncSummary {
    pub backend: SyncBackend,
    pub configured: bool,
    pub installed: bool,
    pub running: bool,
    pub target: String,
    pub last_pushed_session: Option<u32>,
    pub log_tail_path: PathBuf,
}

pub fn summarize(backend: SyncBackend, dir: &Path) -> Option<SyncSummary> {
    match backend {
        SyncBackend::Gh => crate::gh_sync::summarize(dir),
        SyncBackend::Zulip => crate::zulip_sync::summarize(dir),
    }
}

pub fn summarize_all(dir: &Path) -> Vec<SyncSummary> {
    [SyncBackend::Gh, SyncBackend::Zulip]
        .into_iter()
        .filter_map(|b| summarize(b, dir))
        .collect()
}

/// RAII guard for sync daemon pid files. Writes the current PID on
/// construction and unlinks the file on `Drop`, so the pid file is cleaned up
/// even when setup code below the write returns an error via `?`. Without
/// this, a crashed daemon can leave a stale pid file whose PID is later
/// recycled to an unrelated process — `libc::kill(pid, 0)` would then report
/// the daemon as "running" in the hub UI.
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    pub fn create(path: PathBuf) -> Result<Self> {
        std::fs::write(&path, std::process::id().to_string())
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncLoopCommand {
    /// Pull succeeded (or failed transiently); proceed to push the outbox.
    /// Pull and push are independent operations — a transient pull failure
    /// should not block delivery of locally queued outbox messages.
    Send,
    /// Pull detected a condition where send is also unsafe (e.g. the cycle
    /// has no usable client because configuration could not load). Skip send
    /// for this cycle and retry next tick.
    SkipSend,
    /// The backend is in an unrecoverable state (auth failure, missing token,
    /// mis-set-up channel, stream not found). Terminating the loop cleanly
    /// so operators see the reason in the log instead of a silent restart
    /// loop that keeps hitting the same wall.
    Halt { reason: String },
}

/// Outcome of a single send phase. `Continue` is the healthy path (whether
/// push succeeded or hit a transient error). `Halt` signals that push hit an
/// unrecoverable condition (e.g. auth failure) and the loop should exit so
/// operators see a single clear message rather than a silent retry storm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncCycleStatus {
    Continue,
    Halt { reason: String },
}

/// Classification of a sync backend error. Used to decide whether a failure
/// is worth halting the loop over (`AuthOrConfig` — misconfiguration that
/// won't self-heal) or whether the loop should keep going (`Transient` —
/// network blip, rate limit, 5xx).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncErrorKind {
    AuthOrConfig,
    Transient,
}

/// Heuristic classification of anyhow errors from gh/zulip pull/push calls.
/// We don't own the channel layer's error types (subprocess stderr for gh,
/// stringified API errors for zulip), so we match on well-known substrings
/// in the full error chain. Anything unrecognized is `Transient` by default —
/// we'd rather keep running on an unclassified hiccup than halt on one.
pub fn classify_sync_error(err: &anyhow::Error) -> SyncErrorKind {
    let text: String = err.chain().map(|c| format!("{c}\n")).collect();
    let lower = text.to_ascii_lowercase();
    const AUTH_MARKERS: &[&str] = &[
        "http 401",
        "http 403",
        "401 unauthorized",
        "403 forbidden",
        "requires authentication",
        "bad credentials",
        "invalid api key",
        "authentication failed",
        "not authenticated",
        "permission denied",
        "must authenticate",
        "resource not accessible",
    ];
    if AUTH_MARKERS.iter().any(|m| lower.contains(m)) {
        return SyncErrorKind::AuthOrConfig;
    }
    SyncErrorKind::Transient
}

pub trait SyncLoopBackend {
    fn receive(&mut self) -> Result<SyncLoopCommand>;
    fn send(&mut self) -> Result<SyncCycleStatus>;
}

/// Format an outbox message before posting it to an external sync backend.
///
/// Agent replies use the body alone because the external service already
/// shows which integration posted it. System messages are quoted so reports
/// and fallback alerts read as generated status. Other senders keep explicit
/// attribution in the body.
pub fn format_outbox_post(msg: &Message) -> String {
    if msg.from == "agent" {
        msg.body.clone()
    } else if msg.from == "cryochamber" {
        let mut out = format!("> **{}**\n>\n", msg.subject);
        for line in msg.body.lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
        out
    } else {
        format!("**{}** ({})\n\n{}", msg.from, msg.subject, msg.body)
    }
}

pub fn watch_outbox(dir: &Path, tx: Sender<()>) -> Result<notify::RecommendedWatcher> {
    use notify::Watcher;

    let outbox_path = dir.join("messages").join("outbox");
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
    Ok(watcher)
}

pub fn spawn_shutdown_notifier(shutdown: Arc<AtomicBool>, tx: Sender<()>) {
    std::thread::spawn(move || {
        while !shutdown.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
        }
        let _ = tx.send(());
    });
}

pub fn run_sync_loop<B: SyncLoopBackend + ?Sized>(
    label: &str,
    shutdown: Arc<AtomicBool>,
    event_rx: Receiver<()>,
    interval: Duration,
    backend: &mut B,
) -> Result<()> {
    run_sync_loop_with_settle_delay(
        label,
        shutdown,
        event_rx,
        interval,
        Duration::from_millis(200),
        backend,
    )
}

fn run_sync_loop_with_settle_delay<B: SyncLoopBackend + ?Sized>(
    label: &str,
    shutdown: Arc<AtomicBool>,
    event_rx: Receiver<()>,
    interval: Duration,
    settle_delay: Duration,
    backend: &mut B,
) -> Result<()> {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            eprintln!("{label}: shutting down");
            break;
        }

        match backend.receive()? {
            SyncLoopCommand::Send => match backend.send()? {
                SyncCycleStatus::Continue => {}
                SyncCycleStatus::Halt { reason } => {
                    eprintln!("{label}: halting — {reason}");
                    break;
                }
            },
            SyncLoopCommand::SkipSend => {}
            SyncLoopCommand::Halt { reason } => {
                eprintln!("{label}: halting — {reason}");
                break;
            }
        }

        if shutdown.load(Ordering::Relaxed) {
            eprintln!("{label}: shutting down");
            break;
        }

        match event_rx.recv_timeout(interval) {
            Ok(()) => {
                std::thread::sleep(settle_delay);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    eprintln!("{label}: stopped");
    Ok(())
}

fn resolve_cli(backend: SyncBackend) -> Result<std::path::PathBuf> {
    let (env_var, bin_name) = match backend {
        SyncBackend::Gh => ("CRYO_GH_CLI", "cryo-gh"),
        SyncBackend::Zulip => ("CRYO_ZULIP_CLI", "cryo-zulip"),
    };
    if let Ok(p) = std::env::var(env_var) {
        return Ok(std::path::PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sibling = parent.join(bin_name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin_name}"))
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(std::path::PathBuf::from(path));
            }
        }
    }
    anyhow::bail!("{bin_name} binary not found (tried ${env_var}, sibling of current exe, $PATH)");
}

fn run_subcommand(backend: SyncBackend, dir: &Path, sub: &str) -> Result<()> {
    let cli = resolve_cli(backend)?;
    let output = std::process::Command::new(&cli)
        .current_dir(dir)
        .arg(sub)
        .output()
        .with_context(|| format!("Failed to spawn {}", cli.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let truncated: String = stderr.chars().take(500).collect();
        anyhow::bail!(
            "{} {sub} exited with {}: {}",
            cli.display(),
            output.status,
            truncated.trim()
        );
    }
    Ok(())
}

pub fn start(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "sync")
}
pub fn stop(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "unsync")
}
pub fn pull(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "pull")
}
pub fn push(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "push")
}

/// Ground-truth running check — reads the per-backend pid file and verifies
/// the process is alive. The hub uses this to decide what to show in the
/// sync toggle.
pub fn is_running(backend: SyncBackend, dir: &Path) -> bool {
    match backend {
        SyncBackend::Gh => crate::gh_sync::is_sync_running(dir),
        SyncBackend::Zulip => crate::zulip_sync::is_sync_running(dir),
    }
}

/// Poll `is_running` until it matches `expected` or the deadline elapses.
/// Used after start/stop so the HTTP response (and the SSE status event that
/// follows) reflects the settled pid-file state. Without this wait, the hub
/// toggle would flip back to "off" right after a start click — `launchctl
/// load -w` / `launchctl unload -w` return before the daemon has written or
/// cleared its pid file, and nothing re-fetches sync state after that race
/// window closes.
///
/// Returns `true` if `expected` was observed before the deadline.
pub fn wait_for_state(
    backend: SyncBackend,
    dir: &Path,
    expected: bool,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if is_running(backend, dir) == expected {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
#[path = "unit_tests/sync_common.rs"]
mod tests;
