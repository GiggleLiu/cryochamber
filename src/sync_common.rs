//! Shared sync backend abstractions: summary types and sync-loop helpers.

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
    Zulip,
}

impl SyncBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncBackend::Zulip => "zulip",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
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

/// Heuristic classification of anyhow errors from Zulip pull/push calls.
/// We don't own the channel layer's error types (stringified API errors),
/// so we match on well-known substrings in the full error chain. Anything
/// unrecognized is `Transient` by default — we'd rather keep running on an
/// unclassified hiccup than halt on one.
pub fn classify_sync_error(err: &anyhow::Error) -> SyncErrorKind {
    let text: String = err.chain().map(|c| format!("{c}\n")).collect();
    let lower = text.to_ascii_lowercase();
    const AUTH_MARKERS: &[&str] = &[
        // ureq 3.x renders a status error as "http status: 401" (Zulip path).
        "http status: 401",
        "http status: 403",
        // Other stringified auth-error phrasings.
        "http 401",
        "http 403",
        "401 unauthorized",
        "403 forbidden",
        "requires authentication",
        "bad credentials",
        "invalid api key",
        "authentication failed",
        "not authenticated",
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
/// shows which integration posted it. System messages are quoted so fallback
/// alerts read as generated status. Other senders keep explicit
/// attribution in the body.
pub fn format_outbox_post(msg: &Message) -> String {
    match outbox_post_style(msg) {
        OutboxPostStyle::BodyOnly => msg.body.clone(),
        OutboxPostStyle::SystemQuote => format_system_quote(msg),
        OutboxPostStyle::Attributed => {
            format!("**{}** ({})\n\n{}", msg.from, msg.subject, msg.body)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutboxPostStyle {
    BodyOnly,
    SystemQuote,
    Attributed,
}

fn outbox_post_style(msg: &Message) -> OutboxPostStyle {
    match msg.from.as_str() {
        "agent" => OutboxPostStyle::BodyOnly,
        "cryochamber" => OutboxPostStyle::SystemQuote,
        _ => OutboxPostStyle::Attributed,
    }
}

fn format_system_quote(msg: &Message) -> String {
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

/// Push queued outbox messages to the backend in filename (timestamp) order.
///
/// Failure handling is per-message so one bad message can't jam the queue:
/// - A **transient** failure (5xx, network blip, timeout — `Transient`) stops
///   the cycle and returns the error, leaving this and every later message in
///   the outbox so ordering is preserved and they retry next cycle.
/// - A **permanent** failure (`AuthOrConfig`) does not wedge the outbox: we log
///   loudly and archive the offending message anyway, so a body the backend
///   will always reject (too long, invalid chars) can't starve every message
///   queued behind it, then continue with the rest.
///
/// A genuine credential failure normally halts the loop earlier on the pull
/// side, before send runs, so in practice this permanent-skip path handles
/// message-specific rejections rather than account-wide auth. This also assumes
/// a single sync daemon owns the outbox: running GitHub and Zulip sync against
/// the same outbox concurrently is unsupported (GitHub sync is deprecated).
pub fn push_outbox_messages(
    dir: &Path,
    success_message: impl Fn(&str) -> String,
    mut post: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let store = crate::channel::store::MessageStore::new(dir.to_path_buf());
    let messages = store.read_outbox_named()?;
    if messages.is_empty() {
        return Ok(());
    }

    for (filename, msg) in messages {
        let body = format_outbox_post(&msg);
        match post(&body) {
            Ok(()) => {
                eprintln!("{}", success_message(&filename));
                store.archive_outbox(std::slice::from_ref(&filename))?;
            }
            Err(e) => match classify_sync_error(&e) {
                SyncErrorKind::Transient => {
                    // Retriable: stop now and preserve ordering. This message and
                    // everything after it stay queued for the next cycle.
                    return Err(e).with_context(|| format!("Failed to post outbox/{filename}"));
                }
                SyncErrorKind::AuthOrConfig => {
                    // Permanent rejection for this specific message. Archiving it
                    // lets the queue advance instead of re-posting the same doomed
                    // body every cycle and starving every message behind it.
                    eprintln!(
                        "Sync: permanently rejected outbox/{filename} ({e:#}); archiving to unblock the queue"
                    );
                    store.archive_outbox(std::slice::from_ref(&filename))?;
                }
            },
        }
    }

    Ok(())
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

#[cfg(test)]
#[path = "unit_tests/sync_common.rs"]
mod tests;
