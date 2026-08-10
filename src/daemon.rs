// src/daemon.rs
//! Persistent daemon that owns the session lifecycle.
//!
//! Long-running process that:
//! - Sleeps until scheduled wake time
//! - Watches messages/inbox/ for reactive wake
//! - Enforces session timeout
//! - Retries crashed agents with exponential backoff

use anyhow::{Context, Result};
use chrono::{Local, NaiveDateTime};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::CryoConfig;
use crate::state::{self, CryoState};

trait Clock: Send + Sync {
    fn local_now(&self) -> NaiveDateTime;
    fn monotonic_now(&self) -> Instant;
    fn sleep(&self, duration: Duration);
}

struct SystemClock;

impl Clock for SystemClock {
    fn local_now(&self) -> NaiveDateTime {
        Local::now().naive_local()
    }

    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Events the daemon responds to.
#[derive(Debug, PartialEq)]
pub enum DaemonEvent {
    /// New file appeared in a watched inbox/source directory.
    InboxChanged { paths: Vec<PathBuf> },
    /// SIGTERM or SIGINT received.
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitError {
    Timeout,
    Disconnected,
}

trait EventSource {
    fn recv_timeout(&self, timeout: Duration) -> Result<DaemonEvent, WaitError>;
    fn drain_inbox_changed(&self, paths: &mut Vec<PathBuf>);
}

impl EventSource for mpsc::Receiver<DaemonEvent> {
    fn recv_timeout(&self, timeout: Duration) -> Result<DaemonEvent, WaitError> {
        self.recv_timeout(timeout).map_err(Into::into)
    }

    fn drain_inbox_changed(&self, paths: &mut Vec<PathBuf>) {
        while let Ok(DaemonEvent::InboxChanged { paths: more_paths }) = self.try_recv() {
            paths.extend(more_paths);
        }
    }
}

impl From<mpsc::RecvTimeoutError> for WaitError {
    fn from(value: mpsc::RecvTimeoutError) -> Self {
        match value {
            mpsc::RecvTimeoutError::Timeout => Self::Timeout,
            mpsc::RecvTimeoutError::Disconnected => Self::Disconnected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IdleWaitOutcome {
    WakeFromInbox { paths: Vec<PathBuf> },
    WakeFromSchedule,
    Shutdown,
    Disconnected,
    StayIdle,
}

/// Wait for an inbox/shutdown event or for the scheduled wake deadline to arrive.
///
/// The caller typically caps `timeout` far below the real wake deadline so the
/// daemon stays responsive to shutdown and idle-loop housekeeping. Because of
/// that cap, a bare timeout is NOT sufficient evidence the wake fired — this
/// function re-queries `now_fn` on timeout and only reports
/// `WakeFromSchedule` when the deadline has actually been reached. Otherwise
/// it reports `StayIdle` so the caller loops back to sleep.
fn wait_for_idle_event(
    source: &impl EventSource,
    timeout: Duration,
    scheduled_wake: Option<NaiveDateTime>,
    now_fn: impl FnOnce() -> NaiveDateTime,
) -> IdleWaitOutcome {
    match source.recv_timeout(timeout) {
        Ok(DaemonEvent::InboxChanged { mut paths }) => {
            source.drain_inbox_changed(&mut paths);
            IdleWaitOutcome::WakeFromInbox {
                paths: dedupe_paths(paths),
            }
        }
        Ok(DaemonEvent::Shutdown) => IdleWaitOutcome::Shutdown,
        Err(WaitError::Timeout) => match scheduled_wake {
            Some(deadline) if now_fn() >= deadline => IdleWaitOutcome::WakeFromSchedule,
            _ => IdleWaitOutcome::StayIdle,
        },
        Err(WaitError::Disconnected) => IdleWaitOutcome::Disconnected,
    }
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }
    deduped
}

/// True if a `WakeFromInbox` event should be dropped instead of spawning a
/// new session.
///
/// The notify watcher forwards file-arrival events under `messages/inbox`
/// (creates and renames into the directory; atomic tmp+rename writers only
/// produce the latter), including any rename shuffling caused by
/// `MessageStore` archiving a batch the agent claimed mid-session (e.g. after
/// mail rejected its parked hibernate). Those events sit
/// queued on the channel while the session is active and would otherwise
/// look like a fresh wake once the idle loop resumes, even though the inbox
/// is empty. Ignore the wake iff all three hold:
///   (a) `paths` is non-empty — an empty path list carries no evidence about
///       which files arrived, so it must always wake regardless of inbox
///       state (fail open).
///   (b) every path lies inside `inbox_dir` — a mixed batch (e.g. some other
///       watched dir) must still wake.
///   (c) the caller has already determined the inbox is empty. A listing
///       error must be treated as non-empty (fail open — wake rather than
///       risk stranding a message unread).
fn should_ignore_inbox_wake(paths: &[PathBuf], inbox_dir: &Path, inbox_empty: bool) -> bool {
    if paths.is_empty() || !inbox_empty {
        return false;
    }
    paths.iter().all(|path| path_is_within(inbox_dir, path))
}

/// Canonicalize-or-fallback containment check. A stale watcher path usually
/// no longer exists (it was archived away), so canonicalizing it directly
/// fails; canonicalizing only the raw path while the root resolves symlinks
/// (e.g. macOS `/var` → `/private/var` temp dirs) would make the two sides
/// disagree. Resolve the path's parent directory instead — for inbox events
/// that is the inbox dir itself, which still exists — and re-attach the file
/// name. Any residual mismatch returns false, which fails open into a wake.
fn path_is_within(root: &Path, path: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Ok(path) = path.canonicalize() {
        return path.starts_with(&root);
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            parent.join(name).starts_with(&root)
        }
        _ => path.starts_with(&root),
    }
}

/// Watches one or more directories for new files and sends events to a
/// channel. Historically this only watched `messages/inbox/`, but it now
/// accepts an arbitrary list of paths configured via `watch_dirs`.
pub struct InboxWatcher {
    _watcher: RecommendedWatcher,
}

impl InboxWatcher {
    /// Start watching the given directories. Sends `DaemonEvent::InboxChanged`
    /// with changed paths to `tx` when a new file is created in any of them.
    pub fn start(paths: &[PathBuf], tx: mpsc::Sender<DaemonEvent>) -> Result<Self> {
        if paths.is_empty() {
            anyhow::bail!("InboxWatcher requires at least one path");
        }

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                // Atomic writers (tmp-file + rename, e.g. feishu-collaborate's
                // inbox writer) never produce a Create event: the file appears
                // via a rename-into-dir instead. Treat those as new files too.
                let is_new_file = event.kind.is_create()
                    || matches!(
                        event.kind,
                        notify::EventKind::Modify(notify::event::ModifyKind::Name(
                            notify::event::RenameMode::To
                                | notify::event::RenameMode::Both
                                | notify::event::RenameMode::Any,
                        ))
                    );
                if is_new_file {
                    let _ = tx.send(DaemonEvent::InboxChanged { paths: event.paths });
                }
            }
        })
        .context("Failed to create file watcher")?;

        for path in paths {
            watcher
                .watch(path, RecursiveMode::NonRecursive)
                .with_context(|| format!("Failed to watch {}", path.display()))?;
        }

        Ok(Self { _watcher: watcher })
    }
}

/// What the daemon should do after a session completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLoopOutcome {
    PlanComplete,
    Hibernate,
    ValidationFailed { quick_exit: bool, retryable: bool },
}

impl SessionLoopOutcome {
    /// The single source of truth for whether a session ended in a crash /
    /// validation failure. Used to update `CryoState::previous_session_crashed`;
    /// an outer-loop `Err` from `run_one_session` is also a crash but is
    /// handled separately because there is no outcome to ask.
    fn is_crash(&self) -> bool {
        matches!(self, SessionLoopOutcome::ValidationFailed { .. })
    }

    fn is_retryable(&self) -> bool {
        matches!(
            self,
            SessionLoopOutcome::ValidationFailed {
                retryable: true,
                ..
            }
        )
    }

    fn without_retry(self) -> Self {
        match self {
            SessionLoopOutcome::ValidationFailed { quick_exit, .. } => {
                SessionLoopOutcome::ValidationFailed {
                    quick_exit,
                    retryable: false,
                }
            }
            other => other,
        }
    }
}

pub(super) mod dialog;
mod effects;
mod inbox;
mod request;
mod schedule;
mod session;

use effects::{ReplyAuthor, SessionEffects};
use inbox::SessionInboxState;
use schedule::{
    compute_sleep_timeout, decide_next_step, delayed_wake_notice, next_wake_from_todos,
    DaemonBootstrapState, NextStep, SessionRunResult,
};
#[cfg(test)]
use schedule::{detect_delayed_wake, WAKE_TIME_FMT};

#[cfg(test)]
use request::TodoRequest;
use request::{
    effective_linger_secs, handle_dialog_request, handle_receive_request, handle_todo_request,
    resolve_hibernate_request, DaemonRequest, FileTodoEffects, ReceiveRequestOutcome,
    TodoRequestOutcome,
};
#[cfg(test)]
use session::ChildExitStatus;
use session::{ProcessSessionLauncher, SessionLauncher, SessionRuntime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionInterruption {
    Shutdown,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InterruptedSessionDecision {
    outcome: SessionLoopOutcome,
    finish_reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChildExitDecision {
    outcome: SessionLoopOutcome,
    finish_reason: &'static str,
    quick_exit: bool,
    retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Break,
    Idle,
}

struct ActiveSessionContext<'a> {
    cryo_state: &'a CryoState,
    timeout_secs: u64,
    spawn_time: Instant,
    retry_remaining: bool,
    /// Chamber default (secs) for the post-hibernate reply window,
    /// resolved by the launcher as
    /// `config.reply_window.unwrap_or(DEFAULT_REPLY_WINDOW_SECS)`.
    /// 0 = no window.
    reply_window_secs: u64,
}

struct ActiveRequestState<'a> {
    logger: &'a mut crate::log::EventLogger,
    hibernate_outcome: &'a mut Option<SessionLoopOutcome>,
    inbox_state: &'a mut SessionInboxState,
    wait: &'a mut SessionWaitState,
}

/// Reply-window bookkeeping: an accepted `cryo-agent hibernate` held open
/// while the chamber stays quiet. In-memory only, like `SessionInboxState`:
/// a parked hibernate never survives the session. Hibernate is the only call
/// that parks, so the slot needs no kind tag.
struct SessionWaitState {
    /// Monotonic expiry of the currently parked hibernate, if any.
    parked_deadline: Option<Instant>,
    /// When the hibernate parked. On a disconnect-reclaim the session
    /// deadline is shifted by the parked duration — pause semantics — rather
    /// than reset, so a repeatedly-killed client cannot mint fresh session
    /// budgets forever. A mail release keeps the fresh-budget reset.
    parked_since: Option<Instant>,
    /// Chamber default (secs) for the hibernate reply window. 0 = no window.
    reply_window_default_secs: u64,
    /// Set when mail rejects a parked hibernate back into the session; the
    /// loop consumes it to re-arm the session deadline so each conversation
    /// round gets a fresh work budget.
    reset_session_deadline: bool,
}

struct EventLoopMutations<'a> {
    next_wake: &'a mut Option<NaiveDateTime>,
}

/// Retry cadence when `todo.json` shows a past-due pending TODO that
/// `claim_due` could not claim (an I/O failure, e.g. disk full): the wake
/// stays due on disk, so without a pause the event loop would retry the
/// failing write hot.
const TODO_CLAIM_RETRY_DELAY: Duration = Duration::from_secs(60);

/// Number of trailing agent-log lines included in crash fallback messages.
const AGENT_LOG_TAIL_LINES: usize = 10;
/// Cap per included line so one giant line can't bloat the message.
const AGENT_LOG_TAIL_LINE_CHARS: usize = 200;
/// Read at most this many bytes from the end of cryo-agent.log.
const AGENT_LOG_TAIL_READ_BYTES: u64 = 16 * 1024;

/// Strip ANSI escape sequences (CSI/OSC) and control characters from one raw
/// agent-log line so it renders cleanly in an operator-facing message.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    // CSI sequence: parameters end at a final byte in @..~.
                    for c2 in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c2) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC sequence: runs to BEL or ESC \.
                    while let Some(c2) = chars.next() {
                        if c2 == '\u{7}' {
                            break;
                        }
                        if c2 == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c == '\t' {
            out.push(' ');
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// Extract the last `AGENT_LOG_TAIL_LINES` non-empty, sanitized lines from raw
/// agent-log content.
fn agent_log_tail(content: &str) -> Vec<String> {
    let cleaned: Vec<String> = content
        .lines()
        .map(strip_ansi)
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    cleaned
        .into_iter()
        .rev()
        .take(AGENT_LOG_TAIL_LINES)
        .map(|l| {
            if l.chars().count() > AGENT_LOG_TAIL_LINE_CHARS {
                let mut cut: String = l.chars().take(AGENT_LOG_TAIL_LINE_CHARS).collect();
                cut.push('…');
                cut
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Debugging suffix appended to crash fallback messages: the tail of
/// cryo-agent.log in a code fence (a 4-backtick fence survives ``` inside the
/// log). Empty when the log is missing, unreadable, or has no usable lines —
/// the fallback must never get worse because diagnostics are unavailable.
fn crash_debug_suffix(agent_log_path: &Path) -> String {
    let Some(content) = read_file_tail(agent_log_path, AGENT_LOG_TAIL_READ_BYTES) else {
        return String::new();
    };
    let lines = agent_log_tail(&content);
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "\n\nLast agent log output (`cryo-agent.log`):\n````\n{}\n````",
        lines.join("\n")
    )
}

/// Read up to `max_bytes` from the end of a file, lossily decoded.
fn read_file_tail(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > max_bytes {
        file.seek(SeekFrom::End(-(max_bytes as i64))).ok()?;
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn daemon_unanswered_reply_text(clean_hibernate: bool) -> &'static str {
    if clean_hibernate {
        "(daemon: agent hibernated without replying)"
    } else {
        "(daemon: agent crashed before replying)"
    }
}

fn daemon_missing_outbound_text(clean_hibernate: bool) -> &'static str {
    if clean_hibernate {
        "(daemon: agent hibernated without sending anything)"
    } else {
        "(daemon: agent crashed before sending)"
    }
}

fn session_has_retry_blocking_side_effects(inbox_state: &SessionInboxState) -> bool {
    inbox_state.has_claimed_batch() || inbox_state.has_agent_outbound_message()
}

/// Response released to a parked hibernate when new inbox mail arrives inside
/// the reply window: the sleep is rejected, the agent handles the work.
pub(super) const HIBERNATE_LINGER_INBOX_RESPONSE: &str =
    "hibernate rejected: new message(s) arrived in the inbox. Run `cryo-agent receive` to \
     read them, reply with `cryo-agent send`, then run `cryo-agent hibernate` again.";

/// Response released to a parked hibernate when a TODO comes due inside the
/// window: the sleep is granted so the outer loop runs the TODO in a fresh
/// session with a proper wake prompt.
pub(super) const HIBERNATE_LINGER_TODO_DUE_RESPONSE: &str =
    "Hibernating. (A TODO is due; the daemon will run it in a fresh session.)";

/// Retry transient agent-runner failures before surfacing a final daemon
/// fallback. The count is retries after the initial attempt.
const MAX_AGENT_RETRIES: usize = 10;

fn agent_retry_backoff(retry_number: usize) -> Duration {
    debug_assert!(retry_number >= 1);
    Duration::from_secs(1_u64 << retry_number.saturating_sub(1).min(63))
}

fn ipc_protocol_response(protocol_version: u32) -> crate::socket::Response {
    let daemon_version = crate::socket::IPC_PROTOCOL_VERSION;
    if protocol_version == daemon_version {
        return crate::socket::Response {
            ok: true,
            message: format!("IPC protocol {daemon_version}"),
        };
    }

    crate::socket::Response {
        ok: false,
        message: format!(
            "IPC protocol mismatch: daemon uses {daemon_version}, client uses {protocol_version}. \
             Run `cryo restart` after installing matching `cryo` and `cryo-agent` binaries."
        ),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StartupDiagnostics {
    registry_warning: Option<String>,
    watcher_warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatcherStartupNotice<'a> {
    Warning(&'a str),
    Started,
    Silent,
}

/// Resolve the configured watch directories into absolute filesystem paths.
/// Relative entries are interpreted against the chamber directory. Paths that
/// do not currently exist are filtered out; the daemon does not auto-create
/// arbitrary watch targets (other than the canonical `messages/inbox` already
/// scaffolded by `cryo init`).
fn resolve_watch_dirs(chamber_dir: &Path, configured: &[PathBuf]) -> Vec<PathBuf> {
    configured
        .iter()
        .map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                chamber_dir.join(p)
            }
        })
        .filter(|p| p.exists())
        .collect()
}

fn watcher_startup_notice(
    watcher_warning: Option<&str>,
    watcher_started: bool,
) -> WatcherStartupNotice<'_> {
    match (watcher_warning, watcher_started) {
        (Some(warning), _) => WatcherStartupNotice::Warning(warning),
        (None, true) => WatcherStartupNotice::Started,
        (None, false) => WatcherStartupNotice::Silent,
    }
}

#[derive(Debug)]
struct StartupResources<S, W> {
    sock_path: PathBuf,
    server: S,
    watcher: Option<W>,
    diagnostics: StartupDiagnostics,
}

trait StartupPlatform {
    type Server;
    type Watcher;

    fn register_signal_handlers(&self, shutdown: &Arc<AtomicBool>) -> Result<()>;
    fn bind_socket_server(&self, sock_path: &Path) -> Result<Self::Server>;
    fn register_registry(&self, dir: &Path, sock_path: &Path) -> Result<()>;
    fn start_inbox_watcher(
        &self,
        paths: &[PathBuf],
        tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<Self::Watcher>;
}

struct SystemStartupPlatform;

impl StartupPlatform for SystemStartupPlatform {
    type Server = crate::socket::SocketServer;
    type Watcher = InboxWatcher;

    fn register_signal_handlers(&self, shutdown: &Arc<AtomicBool>) -> Result<()> {
        flag::register(SIGTERM, Arc::clone(shutdown))
            .context("Failed to register SIGTERM handler")?;
        flag::register(SIGINT, Arc::clone(shutdown))
            .context("Failed to register SIGINT handler")?;
        Ok(())
    }

    fn bind_socket_server(&self, sock_path: &Path) -> Result<Self::Server> {
        let server = crate::socket::SocketServer::bind(sock_path)?;
        server.set_nonblocking(true)?;
        Ok(server)
    }

    fn register_registry(&self, dir: &Path, sock_path: &Path) -> Result<()> {
        crate::registry::register(dir, Some(sock_path))?;
        Ok(())
    }

    fn start_inbox_watcher(
        &self,
        paths: &[PathBuf],
        tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<Self::Watcher> {
        InboxWatcher::start(paths, tx)
    }
}

fn resolve_interrupted_session(
    interruption: SessionInterruption,
    hibernate_outcome: Option<SessionLoopOutcome>,
) -> InterruptedSessionDecision {
    match (interruption, hibernate_outcome) {
        (SessionInterruption::Shutdown, Some(outcome)) => InterruptedSessionDecision {
            outcome,
            finish_reason: "daemon shutdown — using agent's hibernate outcome",
        },
        (SessionInterruption::Timeout, Some(outcome)) => InterruptedSessionDecision {
            outcome,
            finish_reason: "session timeout — using agent's hibernate outcome",
        },
        (SessionInterruption::Shutdown, None) => InterruptedSessionDecision {
            outcome: SessionLoopOutcome::ValidationFailed {
                quick_exit: false,
                retryable: false,
            },
            finish_reason: "daemon shutdown — agent terminated",
        },
        (SessionInterruption::Timeout, None) => InterruptedSessionDecision {
            outcome: SessionLoopOutcome::ValidationFailed {
                quick_exit: false,
                retryable: false,
            },
            finish_reason: "session timeout — agent killed",
        },
    }
}

const QUICK_EXIT_THRESHOLD: Duration = Duration::from_secs(5);

fn resolve_child_exit(
    hibernate_outcome: Option<SessionLoopOutcome>,
    elapsed: Duration,
    exit_code: Option<i32>,
) -> ChildExitDecision {
    if let Some(outcome) = hibernate_outcome {
        return ChildExitDecision {
            outcome,
            finish_reason: "session complete",
            quick_exit: false,
            retryable: false,
        };
    }

    let quick_exit = elapsed < QUICK_EXIT_THRESHOLD;
    let retryable = exit_code.is_some_and(|code| code != 0) || quick_exit;
    ChildExitDecision {
        outcome: SessionLoopOutcome::ValidationFailed {
            quick_exit,
            retryable,
        },
        finish_reason: "agent exited without hibernate",
        quick_exit,
        retryable,
    }
}

const PREVIOUS_SESSION_CRASH_NOTICE: &str =
    "PREVIOUS SESSION CRASHED: The agent exited without calling \
     `cryo-agent hibernate`. Any inbox messages the previous session \
     received were already archived when they were read. Check \
     `messages/inbox/archive/` if you need to inspect them, then send \
     a human-visible response via \
     `cryo-agent send` if the user is still waiting for a response. \
     Any TODO that triggered the crashed wake has been re-queued \
     with an `(attempt k)` suffix and an exponential delay.";

fn session_prompt_notice(
    delayed_wake: Option<&str>,
    previous_session_crashed: bool,
) -> Option<String> {
    match (delayed_wake, previous_session_crashed) {
        (Some(delayed), true) => Some(format!("{delayed}\n\n{PREVIOUS_SESSION_CRASH_NOTICE}")),
        (Some(delayed), false) => Some(delayed.to_string()),
        (None, true) => Some(PREVIOUS_SESSION_CRASH_NOTICE.to_string()),
        (None, false) => None,
    }
}

/// Persists `CryoState` to disk. Abstracted so tests can inject a stub
/// that fails on demand, covering the "disk write failed mid-loop" paths
/// without relying on filesystem permissions (which behave differently on
/// macOS vs. Linux and vary with parent-directory ownership).
trait StateStore: Send + Sync {
    fn save(&self, path: &Path, state: &CryoState) -> Result<()>;
}

struct FsStateStore;

impl StateStore for FsStateStore {
    fn save(&self, path: &Path, state: &CryoState) -> Result<()> {
        state::save_state(path, state)
    }
}

/// The persistent daemon process.
pub struct Daemon {
    dir: PathBuf,
    state_path: PathBuf,
    log_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    clock: Arc<dyn Clock>,
    launcher: Arc<dyn SessionLauncher>,
    state_store: Arc<dyn StateStore>,
}

impl Daemon {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_deps(
            dir,
            Arc::new(SystemClock),
            Arc::new(ProcessSessionLauncher),
            Arc::new(FsStateStore),
        )
    }

    fn with_deps(
        dir: PathBuf,
        clock: Arc<dyn Clock>,
        launcher: Arc<dyn SessionLauncher>,
        state_store: Arc<dyn StateStore>,
    ) -> Self {
        let state_path = dir.join("timer.json");
        let log_path = dir.join("cryo.log");
        Self {
            dir,
            state_path,
            log_path,
            shutdown: Arc::new(AtomicBool::new(false)),
            clock,
            launcher,
            state_store,
        }
    }

    #[cfg(test)]
    fn new_with_clock(dir: PathBuf, clock: Arc<dyn Clock>) -> Self {
        Self::with_deps(
            dir,
            clock,
            Arc::new(ProcessSessionLauncher),
            Arc::new(FsStateStore),
        )
    }

    /// Test-only constructor: inject both the clock and the session launcher.
    /// Production always uses `ProcessSessionLauncher`; tests pass a
    /// `ScriptedSessionLauncher` to drive the outer event loop without
    /// spawning real subprocesses.
    #[cfg(test)]
    fn new_with_clock_and_launcher(
        dir: PathBuf,
        clock: Arc<dyn Clock>,
        launcher: Arc<dyn SessionLauncher>,
    ) -> Self {
        Self::with_deps(dir, clock, launcher, Arc::new(FsStateStore))
    }

    /// All in-daemon state writes funnel through this, so tests can stub
    /// `StateStore` and all save paths respond consistently.
    fn save_state(&self, cryo_state: &CryoState) -> Result<()> {
        self.state_store.save(&self.state_path, cryo_state)
    }

    fn save_state_or_log(&self, cryo_state: &CryoState, context: &str) {
        if let Err(e) = self.save_state(cryo_state) {
            eprintln!("Daemon: failed to {context}: {e}");
        }
    }

    fn build_bootstrap_state(
        &self,
        cryo_state: &CryoState,
        config: &CryoConfig,
    ) -> DaemonBootstrapState {
        let next_wake = next_wake_from_todos(&self.dir);
        let run_now = cryo_state.session_number == 0
            || next_wake.is_some_and(|w| self.clock.local_now() >= w);

        let watch_dirs = resolve_watch_dirs(&self.dir, &config.watch_dirs);

        DaemonBootstrapState {
            next_wake,
            run_now,
            watch_dirs,
        }
    }

    fn prepare_shutdown_state(&self, cryo_state: &mut CryoState) {
        cryo_state.pid = None;
        cryo_state.instance_id = None;
        cryo_state.session_active = false;
    }

    fn prepare_startup_state(&self, cryo_state: &mut CryoState) {
        cryo_state.pid = Some(std::process::id());
        cryo_state.instance_id = Some(state::new_instance_id());
        // Clear any stale `session_active` left over from a SIGKILL mid-session
        // in a previous run — the hub reads this flag to animate the sidebar
        // dot and should never see "in-session" for a daemon that isn't.
        cryo_state.session_active = false;
    }

    fn prepare_runtime_startup<P: StartupPlatform>(
        &self,
        platform: &P,
        watch_dirs: &[PathBuf],
        tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<StartupResources<P::Server, P::Watcher>> {
        platform.register_signal_handlers(&self.shutdown)?;

        let sock_path = crate::socket::socket_path(&self.dir);
        if let Some(parent) = sock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let server = platform.bind_socket_server(&sock_path)?;

        let registry_warning = platform
            .register_registry(&self.dir, &sock_path)
            .err()
            .map(|e| e.to_string());

        let (watcher, watcher_warning) = if watch_dirs.is_empty() {
            (None, None)
        } else {
            match platform.start_inbox_watcher(watch_dirs, tx) {
                Ok(watcher) => (Some(watcher), None),
                Err(e) => (None, Some(e.to_string())),
            }
        };

        Ok(StartupResources {
            sock_path,
            server,
            watcher,
            diagnostics: StartupDiagnostics {
                registry_warning,
                watcher_warning,
            },
        })
    }

    fn handle_idle_request(
        &self,
        request: crate::socket::Request,
        responder: crate::socket::Responder,
    ) -> Result<()> {
        match DaemonRequest::from(request) {
            DaemonRequest::Ping => {
                let _ = responder.respond(&crate::socket::Response {
                    ok: true,
                    message: "pong".into(),
                });
            }
            DaemonRequest::Hello { protocol_version } => {
                let _ = responder.respond(&ipc_protocol_response(protocol_version));
            }
            DaemonRequest::Todo(todo_request) => {
                let mut effects = FileTodoEffects::new(&self.dir);
                let response =
                    handle_todo_request(todo_request, &mut effects, self.clock.local_now())
                        .into_response();
                let _ = responder.respond(&response);
            }
            // Reading the inbox is only valid inside an active session, which
            // owns a `SessionInboxState` and therefore a reply obligation (the
            // agent's next `send`, or the daemon fallback). When idle there is
            // no such state, so claiming + archiving a batch here would
            // terminally consume it with nobody on the hook to answer —
            // violating invariant 2. Refuse without touching the inbox.
            DaemonRequest::Receive | DaemonRequest::Dialog { .. } => {
                let _ = responder.respond(&crate::socket::Response {
                    ok: false,
                    message:
                        "No active session. Inbox can only be read while the agent is running."
                            .into(),
                });
            }
            DaemonRequest::Send { .. } | DaemonRequest::Hibernate { .. } => {
                let _ = responder.respond(&crate::socket::Response {
                    ok: false,
                    message:
                        "No active session. This command is only valid while the agent is running."
                            .into(),
                });
            }
        }
        Ok(())
    }

    fn service_idle_socket_requests(
        &self,
        server: &crate::socket::SocketServer,
        expected_instance_id: Option<&str>,
    ) {
        loop {
            match server.accept_one(expected_instance_id) {
                Ok(Some((request, responder))) => {
                    if let Err(e) = self.handle_idle_request(request, responder) {
                        eprintln!("Daemon: idle socket request failed: {e}");
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                        if io_err.kind() == std::io::ErrorKind::WouldBlock {
                            break;
                        }
                    }
                    eprintln!("Daemon: socket accept error: {e}");
                    break;
                }
            }
        }
    }

    fn apply_next_step(
        &self,
        step: NextStep,
        state: EventLoopMutations<'_>,
    ) -> Result<LoopControl> {
        match step {
            NextStep::PlanComplete => {
                eprintln!("Daemon: plan complete. Shutting down.");
                Ok(LoopControl::Break)
            }
            NextStep::Hibernate {
                next_wake: refreshed_next_wake,
            } => {
                *state.next_wake = refreshed_next_wake;
                if let Some(w) = *state.next_wake {
                    eprintln!("Daemon: next wake at {}", w.format("%Y-%m-%d %H:%M"));
                } else {
                    eprintln!("Daemon: no pending TODOs, idling");
                }
                Ok(LoopControl::Idle)
            }
        }
    }

    fn sleep_agent_retry_backoff(&self, duration: Duration) -> bool {
        let mut remaining = duration;
        while remaining > Duration::ZERO {
            if self.shutdown.load(Ordering::Relaxed) {
                return false;
            }
            let step = remaining.min(Duration::from_millis(250));
            self.clock.sleep(step);
            remaining = remaining.saturating_sub(step);
        }
        !self.shutdown.load(Ordering::Relaxed)
    }

    /// Run the daemon event loop. Blocks until SIGTERM or plan completion.
    pub fn run(&self) -> Result<()> {
        let mut cryo_state =
            state::load_state(&self.state_path)?.context("No cryochamber state found")?;

        // Guard: refuse to start only if another daemon is both locked AND
        // actually answering on its socket. A stale PID (reboot / PID reuse)
        // that no longer responds is treated as dead so this daemon can take
        // over — the code below mints a fresh pid + instance_id. Bailing on
        // `is_locked` alone could block startup forever behind a PID that now
        // belongs to some unrelated process.
        if state::is_locked(&cryo_state) && crate::daemon_client::daemon_responding(&self.dir) {
            anyhow::bail!(
                "Another daemon is already running (PID: {:?}). Use `cryo cancel` first.",
                cryo_state.pid
            );
        }

        // Load project config from cryo.toml (fall back to defaults for legacy projects)
        let mut config =
            crate::config::load_config(&crate::config::config_path(&self.dir))?.unwrap_or_default();
        config.apply_overrides(&cryo_state);
        let stale_session_active = cryo_state.session_active;
        let recovered_claimed_todos = self.reschedule_claimed_after_crash();
        if stale_session_active || recovered_claimed_todos {
            cryo_state.previous_session_crashed = true;
        }
        cryo_state.session_active = false;
        let bootstrap = self.build_bootstrap_state(&cryo_state, &config);

        // Save PID so other commands can detect the running daemon, mint a
        // new instance_id, and clear any stale session_active from a prior
        // SIGKILL mid-session.
        self.prepare_startup_state(&mut cryo_state);
        self.save_state(&cryo_state)?;

        let (tx, rx) = mpsc::channel();
        let startup = match self.prepare_runtime_startup(
            &SystemStartupPlatform,
            &bootstrap.watch_dirs,
            tx.clone(),
        ) {
            Ok(startup) => startup,
            Err(e) => {
                self.prepare_shutdown_state(&mut cryo_state);
                if let Err(save_err) = self.save_state(&cryo_state) {
                    eprintln!("Daemon: failed to restore state after startup failure: {save_err}");
                }
                return Err(e);
            }
        };

        let sock_path = startup.sock_path;
        let server = startup.server;
        eprintln!("Daemon: socket listening at {}", sock_path.display());
        if let Some(warning) = startup.diagnostics.registry_warning {
            eprintln!("Daemon: failed to update chamber registry: {warning}");
        }

        let watcher_started = startup.watcher.is_some();
        match watcher_startup_notice(
            startup.diagnostics.watcher_warning.as_deref(),
            watcher_started,
        ) {
            WatcherStartupNotice::Warning(warning) => {
                eprintln!("Daemon: failed to start watch_dirs watcher: {warning}");
            }
            WatcherStartupNotice::Started => {
                let label = bootstrap
                    .watch_dirs
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!("Daemon: watching {label} for new files");
            }
            WatcherStartupNotice::Silent => {}
        }
        let _watcher = startup.watcher;

        // Spawn a thread that forwards signals to the event channel,
        // so recv_timeout() unblocks immediately on SIGTERM/SIGINT.
        let shutdown_flag = Arc::clone(&self.shutdown);
        let signal_tx = tx;
        let signal_clock = Arc::clone(&self.clock);
        std::thread::spawn(move || loop {
            signal_clock.sleep(Duration::from_millis(250));
            if shutdown_flag.load(Ordering::Relaxed) {
                let _ = signal_tx.send(DaemonEvent::Shutdown);
                break;
            }
        });

        // The event loop persists final state before
        // returning. All we need to do after is release external OS resources.
        let loop_result = self.run_event_loop(&config, &mut cryo_state, bootstrap, &server, &rx);
        crate::registry::unregister(&self.dir);
        crate::socket::SocketServer::cleanup(&sock_path);
        eprintln!("Daemon: exited cleanly");
        loop_result
    }

    /// The core event loop, extracted so tests can drive it without installing
    /// real signal handlers or inotify watchers.
    ///
    /// Callers are responsible for populating `cryo_state.pid`/`instance_id`,
    /// binding the socket server, and wiring whatever they want on `rx`
    /// (inbox watcher, signal-forwarding thread, scripted events, etc.).
    /// The loop exits on plan completion, explicit shutdown, or channel
    /// disconnection. Final state cleanup happens in the caller.
    fn run_event_loop(
        &self,
        config: &CryoConfig,
        cryo_state: &mut CryoState,
        bootstrap: DaemonBootstrapState,
        server: &crate::socket::SocketServer,
        rx: &mpsc::Receiver<DaemonEvent>,
    ) -> Result<()> {
        let mut next_wake = bootstrap.next_wake;
        let mut run_now = bootstrap.run_now;
        let mut inbox_wake = false;
        let mut inbox_wake_sources = Vec::new();
        let mut scheduled_wake = false;

        // Inbox messages delivered while the daemon was down (after `cryo
        // restart`, a service auto-restart, or a crash-restart) were never seen
        // by the notify watcher, which only reports files created *after* it
        // starts. Left alone they would sit unread until an unrelated TODO
        // fired — forever if none is pending — violating invariant 2 (every
        // inbox message is answered). Treat a non-empty inbox at startup as an
        // inbox wake so the first session processes it, suppressing the
        // delayed-wake notice as for any inbox-triggered wake.
        match crate::message::list_inbox(&self.dir) {
            Ok(files) if !files.is_empty() => {
                run_now = true;
                inbox_wake = true;
            }
            Ok(_) => {}
            Err(e) => eprintln!("Daemon: failed to list inbox at startup: {e}"),
        }

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                eprintln!("Daemon: received shutdown signal");
                break;
            }

            if run_now {
                run_now = false;
                let is_inbox_wake = inbox_wake;
                inbox_wake = false;
                let is_scheduled_wake = scheduled_wake;
                scheduled_wake = false;
                let wake_sources = std::mem::take(&mut inbox_wake_sources);

                // Detect delayed wake: if the scheduled wake time has long passed
                // (e.g. computer was sleeping), notify the agent instead of failing.
                // Skip this check for inbox-triggered wakes — the agent should handle
                // the user's message without a spurious delay warning.
                let delayed_wake =
                    delayed_wake_notice(is_inbox_wake, next_wake, self.clock.local_now());

                // Build provider env for this session
                let active_provider = config.active_provider();
                let provider_env: std::collections::HashMap<String, String> =
                    active_provider.map(|p| p.env.clone()).unwrap_or_default();
                let provider_name = active_provider.map(|p| p.name.as_str());

                // Claim the wake's due TODOs once for the whole retry
                // campaign. TODOs that become due during retry backoff belong
                // to the next wake, not this failed attempt sequence.
                let claimed_due = match self.claim_past_due_todos() {
                    Ok(count) => count,
                    Err(e) if is_scheduled_wake => {
                        // todo.json unreadable/unwritable at wake time. Keep
                        // the stale `next_wake` armed so this wake refires,
                        // paced, until the file heals — resyncing from the
                        // same broken file would yield None and silently
                        // park the chamber forever.
                        eprintln!(
                            "Daemon: failed to claim due TODOs ({e:#}); retrying in {}s",
                            TODO_CLAIM_RETRY_DELAY.as_secs()
                        );
                        self.sleep_agent_retry_backoff(TODO_CLAIM_RETRY_DELAY);
                        continue;
                    }
                    Err(e) => {
                        // Inbox/bootstrap wakes carry their own work;
                        // run the session even though no TODO could be
                        // claimed, so the message still gets answered.
                        eprintln!("Daemon: failed to claim TODO list: {e:#}");
                        0
                    }
                };

                // Scheduled wakes are demand-driven: the cached `next_wake`
                // only decides when to re-check `todo.json`; whether a
                // session runs is decided by what this wake actually claimed
                // (or by inbox files awaiting a receive). A stale cache —
                // however it came to disagree with disk — therefore costs a
                // file read, never an agent session. Bootstrap and
                // inbox wakes are explicit demand and run regardless.
                if is_scheduled_wake && claimed_due == 0 {
                    // Fail open on a listing error: skipping is only safe
                    // when the inbox is *known* empty (invariant 2).
                    let inbox_has_files = crate::message::list_inbox(&self.dir)
                        .map(|files| !files.is_empty())
                        .unwrap_or(true);
                    if !inbox_has_files {
                        next_wake = next_wake_from_todos(&self.dir);
                        eprintln!(
                            "Daemon: scheduled wake found no due TODO and an empty inbox — \
                             nothing to run; next wake: {}",
                            next_wake
                                .map(|t| t.to_string())
                                .unwrap_or_else(|| "none".into()),
                        );
                        continue;
                    }
                }

                let mut retries_used = 0usize;
                let session_result = loop {
                    cryo_state.session_number += 1;
                    cryo_state.session_active = true;
                    self.save_state_or_log(cryo_state, "persist session-active state");

                    let retry_remaining = retries_used < MAX_AGENT_RETRIES;
                    match self.run_one_session(
                        config,
                        cryo_state,
                        server,
                        delayed_wake.as_deref(),
                        &wake_sources,
                        &provider_env,
                        provider_name,
                        retry_remaining,
                    ) {
                        Ok(outcome) if outcome.is_retryable() && retry_remaining => {
                            cryo_state.previous_session_crashed = false;
                            cryo_state.session_active = false;
                            self.save_state_or_log(
                                cryo_state,
                                "persist retryable failed session state",
                            );

                            retries_used += 1;
                            let delay = agent_retry_backoff(retries_used);
                            eprintln!(
                                "Daemon: retryable agent failure; retrying in {}s ({}/{})",
                                delay.as_secs(),
                                retries_used,
                                MAX_AGENT_RETRIES
                            );
                            if !self.sleep_agent_retry_backoff(delay) {
                                eprintln!("Daemon: received shutdown signal");
                                self.write_deferred_retry_failure_reply();
                                cryo_state.previous_session_crashed = outcome.is_crash();
                                if outcome.is_crash() {
                                    self.reschedule_claimed_after_crash();
                                } else {
                                    self.complete_claimed_todos();
                                }
                                self.save_state_or_log(
                                    cryo_state,
                                    "persist interrupted retry state",
                                );
                                break Ok(outcome);
                            }
                        }
                        Ok(outcome) => {
                            // Single source of truth: outcome decides crash-status.
                            cryo_state.previous_session_crashed = outcome.is_crash();
                            cryo_state.session_active = false;
                            if outcome.is_crash() {
                                self.reschedule_claimed_after_crash();
                            } else {
                                self.complete_claimed_todos();
                            }
                            // Persist session number only after successful completion
                            self.save_state_or_log(cryo_state, "persist completed session state");
                            break Ok(outcome);
                        }
                        Err(e) => {
                            cryo_state.session_number -= 1;
                            cryo_state.session_active = false;
                            cryo_state.previous_session_crashed = true;
                            self.reschedule_claimed_after_crash();
                            self.save_state_or_log(cryo_state, "persist failed session state");
                            eprintln!("Daemon: session failed: {e}");
                            break Err(());
                        }
                    }
                };

                let refreshed_next_wake = match session_result.as_ref() {
                    Ok(SessionLoopOutcome::PlanComplete) => next_wake,
                    _ => next_wake_from_todos(&self.dir),
                };
                let session_result_ref = match &session_result {
                    Ok(outcome) => SessionRunResult::Outcome(outcome),
                    Err(()) => SessionRunResult::Error,
                };
                let step = decide_next_step(session_result_ref, refreshed_next_wake);
                match self.apply_next_step(
                    step,
                    EventLoopMutations {
                        next_wake: &mut next_wake,
                    },
                )? {
                    LoopControl::Break => break,
                    LoopControl::Idle => {}
                }
            }

            let expected_instance_id = cryo_state.instance_id.as_deref();
            self.service_idle_socket_requests(server, expected_instance_id);

            // Wait for next event
            let timeout = compute_sleep_timeout(next_wake, self.clock.local_now())
                .min(Duration::from_millis(250));

            match wait_for_idle_event(rx, timeout, next_wake, || self.clock.local_now()) {
                IdleWaitOutcome::WakeFromInbox { paths } => {
                    let inbox_dir = self.dir.join("messages").join("inbox");
                    let inbox_empty = crate::channel::store::MessageStore::new(self.dir.clone())
                        .list_inbox_filenames()
                        .map(|files| files.is_empty())
                        .unwrap_or(false);
                    if should_ignore_inbox_wake(&paths, &inbox_dir, inbox_empty) {
                        eprintln!("Daemon: ignoring stale inbox events (inbox empty)");
                    } else {
                        eprintln!("Daemon: inbox changed, waking up");
                        run_now = true;
                        inbox_wake = true;
                        inbox_wake_sources = paths;
                    }
                }
                IdleWaitOutcome::WakeFromSchedule => {
                    eprintln!("Daemon: scheduled wake time reached");
                    run_now = true;
                    scheduled_wake = true;
                }
                IdleWaitOutcome::Shutdown => break,
                IdleWaitOutcome::StayIdle => {}
                IdleWaitOutcome::Disconnected => {
                    eprintln!("Daemon: event channel disconnected");
                    break;
                }
            }
        }

        // Persist pid=None so external observers (e.g. the hub, `cryo status`)
        // see a consistent shutdown state.
        self.prepare_shutdown_state(cryo_state);
        self.save_state_or_log(cryo_state, "save final state");

        Ok(())
    }

    /// Delegate to the injected `SessionLauncher`.
    ///
    /// In production the launcher is `ProcessSessionLauncher`, which spawns a
    /// real agent. In tests a `ScriptedSessionLauncher` returns canned
    /// outcomes so the outer event loop can be exercised without wall-clock
    /// delays or subprocess management.
    #[allow(clippy::too_many_arguments)]
    fn run_one_session(
        &self,
        config: &CryoConfig,
        cryo_state: &CryoState,
        server: &crate::socket::SocketServer,
        delayed_wake: Option<&str>,
        wake_sources: &[PathBuf],
        provider_env: &std::collections::HashMap<String, String>,
        provider_name: Option<&str>,
        retry_remaining: bool,
    ) -> Result<SessionLoopOutcome> {
        self.launcher.run_session(
            self,
            config,
            cryo_state,
            server,
            delayed_wake,
            wake_sources,
            provider_env,
            provider_name,
            retry_remaining,
        )
    }

    fn handle_active_request(
        &self,
        request: crate::socket::Request,
        runtime: &mut impl SessionRuntime,
        effects: &mut impl SessionEffects,
        state: ActiveRequestState<'_>,
    ) -> Result<()> {
        match DaemonRequest::from(request) {
            DaemonRequest::Ping => {
                runtime.respond(true, "pong".into())?;
            }
            DaemonRequest::Hello { protocol_version } => {
                let response = ipc_protocol_response(protocol_version);
                runtime.respond(response.ok, response.message)?;
            }
            DaemonRequest::Send { text, question } => {
                let has_claimed_batch = state.inbox_state.has_claimed_batch();
                match effects.write_reply(
                    ReplyAuthor::Agent,
                    &text,
                    self.clock.local_now(),
                    question,
                ) {
                    Ok(()) => {
                        if has_claimed_batch {
                            state.inbox_state.complete_agent_send();
                        } else {
                            state.inbox_state.record_status_send();
                        }
                        // `send --stdin` bodies end with the heredoc's final
                        // newline; trim it so the log line doesn't render a
                        // meaningless trailing ⏎ inside the quotes.
                        state
                            .logger
                            .log_event(&format!("send: \"{}\"", text.trim_end()))?;
                        runtime.respond(true, "Message sent".into())?;
                    }
                    Err(e) => {
                        state.logger.log_event(&format!("send failed: {e}"))?;
                        runtime.respond(false, format!("Failed to write message: {e}"))?;
                    }
                }
            }
            DaemonRequest::Hibernate {
                complete,
                exit_code,
                summary,
            } => {
                let has_due_todo = effects
                    .next_valid_todo_wake()
                    .is_some_and(|wake| wake <= self.clock.local_now());
                let decision = resolve_hibernate_request(
                    complete,
                    exit_code,
                    summary.as_deref(),
                    effects.has_pending_todo_with_valid_wake(),
                    effects.has_unread_inbox(),
                    has_due_todo,
                );
                state.logger.log_event(&decision.log_event)?;
                let linger_secs = if decision.outcome == Some(SessionLoopOutcome::Hibernate) {
                    effective_linger_secs(state.wait.reply_window_default_secs)
                } else {
                    0
                };
                if linger_secs > 0 && state.wait.parked_deadline.is_none() {
                    // Quiet chamber + open window: hold the hibernate instead
                    // of granting it. The parked responder is answered by the
                    // linger tick — with new work (ok=false) or with sleep
                    // (ok=true on TODO-due / expiry).
                    *state.hibernate_outcome = decision.outcome;
                    runtime.park()?;
                    let now_mono = self.clock.monotonic_now();
                    state.wait.parked_deadline = Some(now_mono + Duration::from_secs(linger_secs));
                    state.wait.parked_since = Some(now_mono);
                    state
                        .logger
                        .log_event(&format!("hibernate: parked ({linger_secs}s reply window)"))?;
                } else {
                    if let Some(outcome) = decision.outcome {
                        *state.hibernate_outcome = Some(outcome);
                    }
                    runtime.respond(decision.response_ok, decision.response_message.into())?;
                }
            }
            DaemonRequest::Receive => {
                if state.inbox_state.has_claimed_batch() {
                    runtime.respond(
                        false,
                        "receive refused: send a message for the current inbox batch before receiving again."
                            .into(),
                    )?;
                    return Ok(());
                }

                let ReceiveRequestOutcome {
                    ok,
                    message,
                    log_event,
                    claimed_filenames,
                } = handle_receive_request(effects);
                let response = runtime.respond(ok, message);
                if ok {
                    if let Err(e) = response {
                        state.inbox_state.record_claimed_batch(&claimed_filenames);
                        return Err(e);
                    }
                    state.inbox_state.record_claimed_batch(&claimed_filenames);
                } else {
                    response?;
                }
                if let Some(event) = log_event {
                    state.logger.log_event(&event)?;
                }
            }
            DaemonRequest::Dialog { filter } => {
                let outcome =
                    handle_dialog_request(filter, state.inbox_state.claimed_filenames(), effects);
                let response = runtime.respond(outcome.ok, outcome.message.clone());
                if !outcome.claimed_filenames.is_empty() {
                    state
                        .inbox_state
                        .record_claimed_batch(&outcome.claimed_filenames);
                }
                response?;
                if let Some(event) = outcome.log_event {
                    state.logger.log_event(&event)?;
                }
            }
            DaemonRequest::Todo(todo_request) => {
                let TodoRequestOutcome {
                    ok,
                    message,
                    log_event,
                } = handle_todo_request(todo_request, effects, self.clock.local_now());
                if let Some(event) = log_event {
                    state.logger.log_event(&event)?;
                }
                runtime.respond(ok, message)?;
            }
        }
        Ok(())
    }

    fn finalize_human_replies(
        &self,
        effects: &mut impl SessionEffects,
        logger: &mut crate::log::EventLogger,
        inbox_state: &mut SessionInboxState,
        clean_hibernate: bool,
        suppress_missing_outbound: bool,
    ) {
        // On a crash, attach the agent-log tail so the operator sees *why*
        // without shelling into the chamber. A clean hibernate that merely
        // forgot to reply gets no log dump — nothing crashed.
        let debug_suffix = if clean_hibernate {
            String::new()
        } else {
            crash_debug_suffix(&crate::log::agent_log_path(&self.dir))
        };
        let mut daemon_wrote_reply = false;
        let message_count = inbox_state.claimed_message_count();
        if message_count > 0 {
            let text = format!(
                "{}{debug_suffix}",
                daemon_unanswered_reply_text(clean_hibernate)
            );
            match effects.write_reply(ReplyAuthor::Daemon, &text, self.clock.local_now(), false) {
                Ok(()) => {
                    if let Err(e) = logger.log_event(&format!(
                        "daemon reply: {} unanswered inbox message{} [{}]",
                        message_count,
                        if message_count == 1 { "" } else { "s" },
                        inbox_state.claimed_filenames().join(", "),
                    )) {
                        eprintln!("Daemon: failed to log daemon reply: {e}");
                    }
                    inbox_state.complete_daemon_fallback();
                    daemon_wrote_reply = true;
                }
                Err(e) => {
                    eprintln!(
                        "Daemon: failed to write daemon reply for unanswered inbox messages: {e:#}"
                    );
                    if let Err(log_err) = logger.log_event(&format!("daemon reply failed: {e:#}")) {
                        eprintln!("Daemon: failed to log daemon reply failure: {log_err}");
                    }
                }
            }
        }

        if message_count == 0
            && !inbox_state.has_agent_outbound_message()
            && !daemon_wrote_reply
            && !suppress_missing_outbound
        {
            match effects.write_reply(
                ReplyAuthor::Daemon,
                &format!(
                    "{}{debug_suffix}",
                    daemon_missing_outbound_text(clean_hibernate)
                ),
                self.clock.local_now(),
                false,
            ) {
                Ok(()) => {
                    if let Err(e) =
                        logger.log_event("daemon reply: no outbound message sent by agent")
                    {
                        eprintln!("Daemon: failed to log daemon status reply: {e}");
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Daemon: failed to write daemon status for session without outbound message: {e:#}"
                    );
                    if let Err(log_err) =
                        logger.log_event(&format!("daemon status reply failed: {e:#}"))
                    {
                        eprintln!("Daemon: failed to log daemon status reply failure: {log_err}");
                    }
                }
            }
        }
    }

    fn write_deferred_retry_failure_reply(&self) {
        let text = format!(
            "{}{}",
            daemon_missing_outbound_text(false),
            crash_debug_suffix(&crate::log::agent_log_path(&self.dir))
        );
        let mut effects = effects::FsSessionEffects::new(&self.dir);
        if let Err(e) =
            effects.write_reply(ReplyAuthor::Daemon, &text, self.clock.local_now(), false)
        {
            eprintln!("Daemon: failed to write deferred retry failure reply: {e:#}");
        }
    }

    /// Best-effort release of a parked hibernate when the session ends for
    /// reasons other than the reply window's own resolutions (mail, due TODO,
    /// expiry). Never fails the caller: the blocked cryo-agent may already be
    /// dead.
    fn release_parked_call(
        runtime: &mut impl SessionRuntime,
        wait_state: &mut SessionWaitState,
        logger: &mut crate::log::EventLogger,
    ) {
        if wait_state.parked_deadline.take().is_some() {
            let _ = runtime.respond_parked(true, "Hibernating.".into());
            let _ = logger.log_event("linger: interrupted — hibernate accepted");
        }
    }

    fn drive_active_session(
        &self,
        runtime: &mut impl SessionRuntime,
        effects: &mut impl SessionEffects,
        context: ActiveSessionContext<'_>,
        mut logger: crate::log::EventLogger,
    ) -> Result<SessionLoopOutcome> {
        let mut deadline = if context.timeout_secs > 0 {
            Some(context.spawn_time + Duration::from_secs(context.timeout_secs))
        } else {
            None
        };
        let mut wait_state = SessionWaitState {
            parked_deadline: None,
            parked_since: None,
            reply_window_default_secs: context.reply_window_secs,
            reset_session_deadline: false,
        };

        let mut hibernate_outcome: Option<SessionLoopOutcome> = None;
        let mut inbox_state = SessionInboxState::new();
        let expected_instance_id = context.cryo_state.instance_id.as_deref();

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                Self::release_parked_call(runtime, &mut wait_state, &mut logger);
                runtime.terminate();
                let clean_hibernate = hibernate_outcome.is_some();
                let decision = resolve_interrupted_session(
                    SessionInterruption::Shutdown,
                    hibernate_outcome.take(),
                );
                self.finalize_human_replies(
                    effects,
                    &mut logger,
                    &mut inbox_state,
                    clean_hibernate,
                    false,
                );
                logger.finish(decision.finish_reason)?;
                return Ok(decision.outcome);
            }

            if wait_state.parked_deadline.is_none() {
                if let Some(d) = deadline {
                    if self.clock.monotonic_now() >= d {
                        eprintln!(
                            "Daemon: session timeout ({}s) — killing agent",
                            context.timeout_secs
                        );
                        runtime.terminate();
                        let clean_hibernate = hibernate_outcome.is_some();
                        let decision = resolve_interrupted_session(
                            SessionInterruption::Timeout,
                            hibernate_outcome.take(),
                        );
                        self.finalize_human_replies(
                            effects,
                            &mut logger,
                            &mut inbox_state,
                            clean_hibernate,
                            false,
                        );
                        logger.finish(decision.finish_reason)?;
                        return Ok(decision.outcome);
                    }
                }
            }

            match runtime.accept_request(expected_instance_id) {
                Ok(Some(request)) => {
                    if let Err(e) = self.handle_active_request(
                        request,
                        runtime,
                        effects,
                        ActiveRequestState {
                            logger: &mut logger,
                            hibernate_outcome: &mut hibernate_outcome,
                            inbox_state: &mut inbox_state,
                            wait: &mut wait_state,
                        },
                    ) {
                        Self::release_parked_call(runtime, &mut wait_state, &mut logger);
                        let clean_hibernate = hibernate_outcome.is_some();
                        self.finalize_human_replies(
                            effects,
                            &mut logger,
                            &mut inbox_state,
                            clean_hibernate,
                            false,
                        );
                        logger.finish(&format!("error handling agent request: {e}"))?;
                        return Err(e);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("Daemon: socket accept error: {e}");
                }
            }

            match runtime.try_wait() {
                Ok(Some(status)) => {
                    let elapsed = self
                        .clock
                        .monotonic_now()
                        .saturating_duration_since(context.spawn_time);
                    let exit_code = status.code;
                    logger.log_event(&format!(
                        "agent exited (code {})",
                        exit_code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "signal".into())
                    ))?;

                    let clean_hibernate = hibernate_outcome.is_some();
                    let decision = resolve_child_exit(hibernate_outcome.take(), elapsed, exit_code);
                    if decision.quick_exit {
                        let elapsed_s = format!("{:.1}s", elapsed.as_secs_f32());
                        eprintln!(
                            "Daemon: agent exited in {elapsed_s} without hibernating — possible causes:\n  \
                             - Missing or invalid API key\n  \
                             - Agent command misconfigured (try running it manually)\n  \
                             - Check cryo-agent.log for details"
                        );
                        logger.log_event(&format!(
                            "quick exit detected ({elapsed_s} without hibernate)"
                        ))?;
                    }
                    Self::release_parked_call(runtime, &mut wait_state, &mut logger);
                    let mut outcome = decision.outcome;
                    if decision.retryable && session_has_retry_blocking_side_effects(&inbox_state) {
                        logger.log_event(
                            "retry disabled: session already sent or received messages",
                        )?;
                        outcome = outcome.without_retry();
                    }
                    let suppress_missing_outbound = context.retry_remaining
                        && outcome.is_retryable()
                        && !inbox_state.has_claimed_batch()
                        && !inbox_state.has_agent_outbound_message();
                    if suppress_missing_outbound {
                        logger.log_event(
                            "daemon reply deferred: retryable agent failure, retry remains",
                        )?;
                    }
                    self.finalize_human_replies(
                        effects,
                        &mut logger,
                        &mut inbox_state,
                        clean_hibernate,
                        suppress_missing_outbound,
                    );
                    logger.finish(decision.finish_reason)?;
                    return Ok(outcome);
                }
                Ok(None) => {}
                Err(e) => {
                    Self::release_parked_call(runtime, &mut wait_state, &mut logger);
                    let clean_hibernate = hibernate_outcome.is_some();
                    self.finalize_human_replies(
                        effects,
                        &mut logger,
                        &mut inbox_state,
                        clean_hibernate,
                        false,
                    );
                    logger.finish(&format!("error checking agent: {e}"))?;
                    return Err(e.into());
                }
            }

            if let Some(window_deadline) = wait_state.parked_deadline {
                // Reply-window linger. Notice-only: never claims a batch, so a
                // dead client here loses nothing.
                if runtime.reclaim_parked_if_disconnected() {
                    wait_state.parked_deadline = None;
                    // Pause, don't reset: the recorded hibernate stands, so if
                    // the agent exits now the session ends cleanly, and a
                    // repeatedly-killed client cannot mint fresh session budgets.
                    if let (Some(d), Some(since)) = (deadline, wait_state.parked_since.take()) {
                        deadline =
                            Some(d + self.clock.monotonic_now().saturating_duration_since(since));
                    }
                    if let Err(e) =
                        logger.log_event("linger: client disconnected — hibernate stands")
                    {
                        // A log failure must not skip finalization of
                        // already-claimed batches.
                        let clean_hibernate = hibernate_outcome.is_some();
                        self.finalize_human_replies(
                            effects,
                            &mut logger,
                            &mut inbox_state,
                            clean_hibernate,
                            false,
                        );
                        let _ = logger.finish(&format!("error logging linger release: {e}"));
                        return Err(e);
                    }
                } else if effects.has_unread_inbox() {
                    // New mail inside the window: reject the parked hibernate
                    // so the same LLM context handles it. The earlier
                    // hibernate no longer covers this work — clear it so a
                    // crash from here is real.
                    wait_state.parked_deadline = None;
                    wait_state.parked_since = None;
                    wait_state.reset_session_deadline = true;
                    hibernate_outcome = None;
                    if let Err(e) = logger.log_event("linger: released — inbox message(s) waiting")
                    {
                        let _ =
                            runtime.respond_parked(false, HIBERNATE_LINGER_INBOX_RESPONSE.into());
                        self.finalize_human_replies(
                            effects,
                            &mut logger,
                            &mut inbox_state,
                            hibernate_outcome.is_some(),
                            false,
                        );
                        let _ = logger.finish(&format!("error logging linger release: {e}"));
                        return Err(e);
                    }
                    // Best-effort: a dead client must not skip finalization.
                    // The child-exit path handles the rest.
                    if runtime
                        .respond_parked(false, HIBERNATE_LINGER_INBOX_RESPONSE.into())
                        .is_err()
                    {
                        let _ = logger.log_event("linger: parked respond failed after release");
                    }
                } else if effects
                    .next_valid_todo_wake()
                    .is_some_and(|wake| wake <= self.clock.local_now())
                {
                    // A TODO came due: end the window, grant the sleep. The
                    // outer loop claims the TODO and runs it in a fresh
                    // session with a proper wake prompt.
                    wait_state.parked_deadline = None;
                    wait_state.parked_since = None;
                    if let Err(e) =
                        logger.log_event("linger: released — TODO due, hibernate accepted")
                    {
                        let _ =
                            runtime.respond_parked(true, HIBERNATE_LINGER_TODO_DUE_RESPONSE.into());
                        let clean_hibernate = hibernate_outcome.is_some();
                        self.finalize_human_replies(
                            effects,
                            &mut logger,
                            &mut inbox_state,
                            clean_hibernate,
                            false,
                        );
                        let _ = logger.finish(&format!("error logging linger release: {e}"));
                        return Err(e);
                    }
                    if runtime
                        .respond_parked(true, HIBERNATE_LINGER_TODO_DUE_RESPONSE.into())
                        .is_err()
                    {
                        let _ = logger.log_event("linger: parked respond failed after release");
                    }
                } else if self.clock.monotonic_now() >= window_deadline {
                    wait_state.parked_deadline = None;
                    wait_state.parked_since = None;
                    if let Err(e) = logger.log_event("linger: expired — hibernate accepted") {
                        let _ = runtime.respond_parked(true, "Hibernating.".into());
                        let clean_hibernate = hibernate_outcome.is_some();
                        self.finalize_human_replies(
                            effects,
                            &mut logger,
                            &mut inbox_state,
                            clean_hibernate,
                            false,
                        );
                        let _ = logger.finish(&format!("error logging linger expiry: {e}"));
                        return Err(e);
                    }
                    if runtime.respond_parked(true, "Hibernating.".into()).is_err() {
                        let _ = logger.log_event("linger: parked respond failed after expiry");
                    }
                }
                // Quiet and inside the window: keep lingering.
            }
            if wait_state.reset_session_deadline {
                wait_state.reset_session_deadline = false;
                deadline = if context.timeout_secs > 0 {
                    Some(self.clock.monotonic_now() + Duration::from_secs(context.timeout_secs))
                } else {
                    None
                };
            }

            if hibernate_outcome.is_some() {
                self.clock.sleep(Duration::from_millis(100));
                continue;
            }

            self.clock.sleep(Duration::from_millis(100));
        }
    }

    fn get_task(&self) -> Option<String> {
        crate::log::parse_latest_session_task(&self.log_path)
            .ok()
            .flatten()
    }

    /// Claim past-due pending TODOs so the prompt can show them as in-flight
    /// while the scheduler ignores them. Load/save failures are swallowed and
    /// reported to stderr — TODO bookkeeping must never abort the session loop.
    /// Claim all past-due TODOs for the upcoming session and return how many
    /// were claimed. Errors propagate: the caller must distinguish "nothing
    /// due" (a valid skip for a scheduled wake) from "cannot read/write
    /// todo.json" (must not silently drop the wake).
    fn claim_past_due_todos(&self) -> Result<usize> {
        let now = self.clock.local_now();
        let items = crate::todo::TodoFile::new(self.dir.join("todo.json")).claim_due(&now)?;
        if !items.is_empty() {
            eprintln!("Daemon: claimed {} due TODO(s)", items.len());
        }
        Ok(items.len())
    }

    /// Mark all claimed TODOs as done after a successful session. The claim is
    /// the session's in-flight marker; success makes it terminal.
    fn complete_claimed_todos(&self) {
        match crate::todo::TodoFile::new(self.dir.join("todo.json")).complete_claimed() {
            Ok(items) if !items.is_empty() => {
                eprintln!("Daemon: completed {} claimed TODO(s)", items.len());
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Daemon: failed to complete claimed TODOs: {e}");
            }
        }
    }

    /// Re-inject claimed TODOs after a crashed session. Each item's text gains
    /// a ` (attempt k)` suffix (or its existing suffix is bumped) and its `at`
    /// becomes `now + 2^k` minutes, capped at 1 day.
    fn reschedule_claimed_after_crash(&self) -> bool {
        let now = self.clock.local_now();
        let ids = match crate::todo::TodoFile::new(self.dir.join("todo.json"))
            .reschedule_claimed_after_crash(now)
        {
            Ok(ids) => ids,
            Err(e) => {
                eprintln!("Daemon: failed to reschedule TODO list: {e}");
                return false;
            }
        };
        if ids.is_empty() {
            return false;
        }
        eprintln!(
            "Daemon: rescheduled {} claimed TODO(s) after crash (new ids: {:?})",
            ids.len(),
            ids,
        );
        true
    }
}

#[cfg(test)]
#[path = "unit_tests/daemon.rs"]
mod tests;

#[cfg(test)]
#[path = "unit_tests/daemon/dialog.rs"]
mod dialog_tests;

#[cfg(test)]
#[path = "unit_tests/daemon/request.rs"]
mod request_tests;

#[cfg(test)]
#[path = "unit_tests/daemon/session.rs"]
mod session_tests;

#[cfg(test)]
#[path = "unit_tests/daemon_properties.rs"]
mod property_tests;
