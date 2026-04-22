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
use signal_hook::consts::{SIGINT, SIGTERM, SIGUSR1};
use signal_hook::flag;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::CryoConfig;
use crate::fallback::FallbackAction;
use crate::state::{self, CryoState, InFlightFallback, PendingFallbackState};

/// Format for parsing TODO `at` timestamps (minute precision, no seconds).
const WAKE_TIME_FMT: &str = "%Y-%m-%dT%H:%M";
const FALLBACK_TIME_FMT: &str = "%Y-%m-%dT%H:%M:%S";

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
    /// New file appeared in messages/inbox/.
    InboxChanged,
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
    fn drain_inbox_changed(&self);
}

impl EventSource for mpsc::Receiver<DaemonEvent> {
    fn recv_timeout(&self, timeout: Duration) -> Result<DaemonEvent, WaitError> {
        self.recv_timeout(timeout).map_err(Into::into)
    }

    fn drain_inbox_changed(&self) {
        while matches!(self.try_recv(), Ok(DaemonEvent::InboxChanged)) {}
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleWaitOutcome {
    WakeFromInbox,
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
        Ok(DaemonEvent::InboxChanged) => {
            source.drain_inbox_changed();
            IdleWaitOutcome::WakeFromInbox
        }
        Ok(DaemonEvent::Shutdown) => IdleWaitOutcome::Shutdown,
        Err(WaitError::Timeout) => match scheduled_wake {
            Some(deadline) if now_fn() >= deadline => IdleWaitOutcome::WakeFromSchedule,
            _ => IdleWaitOutcome::StayIdle,
        },
        Err(WaitError::Disconnected) => IdleWaitOutcome::Disconnected,
    }
}

/// Tracks retry state with exponential backoff.
#[derive(Debug)]
pub struct RetryState {
    pub attempt: u32,
    pub max_retries: u32,
    pub provider_index: usize,
    provider_count: usize,
}

impl RetryState {
    pub fn new(max_retries: u32, provider_count: usize) -> Self {
        Self {
            attempt: 0,
            max_retries,
            provider_index: 0,
            provider_count,
        }
    }

    /// Calculate backoff duration for current attempt.
    /// Doubles each time: 5s, 10s, 20s, ..., capped at 3600s (1 hour).
    /// Always returns a duration (retries indefinitely with backoff).
    pub fn next_backoff(&self) -> Duration {
        let secs = 5u64.checked_shl(self.attempt).unwrap_or(3600).min(3600);
        Duration::from_secs(secs)
    }

    pub fn record_failure(&mut self) {
        self.attempt += 1;
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
        self.provider_index = 0;
    }

    pub fn exhausted(&self) -> bool {
        self.attempt >= self.max_retries
    }

    /// Advance to the next provider. Returns true if we wrapped back to index 0
    /// (all providers have been tried in this cycle). Resets retry attempt counter.
    pub fn rotate_provider(&mut self) -> bool {
        if self.provider_count <= 1 {
            return true; // can't rotate with 0 or 1 provider
        }
        self.provider_index = (self.provider_index + 1) % self.provider_count;
        self.attempt = 0;
        self.provider_index == 0 // wrapped
    }
}

/// Watches `messages/inbox/` for new files and sends events to a channel.
pub struct InboxWatcher {
    _watcher: RecommendedWatcher,
}

impl InboxWatcher {
    /// Start watching the inbox directory. Sends `DaemonEvent::InboxChanged`
    /// to `tx` when a new file is created.
    pub fn start(inbox_path: &Path, tx: mpsc::Sender<DaemonEvent>) -> Result<Self> {
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event.kind.is_create() {
                    let _ = tx.send(DaemonEvent::InboxChanged);
                }
            }
        })
        .context("Failed to create file watcher")?;

        watcher
            .watch(inbox_path, RecursiveMode::NonRecursive)
            .with_context(|| format!("Failed to watch {}", inbox_path.display()))?;

        Ok(Self { _watcher: watcher })
    }
}

/// What the daemon should do after a session completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLoopOutcome {
    PlanComplete,
    Hibernate { fallback: Option<FallbackAction> },
    ValidationFailed { quick_exit: bool },
}

impl SessionLoopOutcome {
    /// The single source of truth for whether a session ended in a crash /
    /// validation failure. Used to update `CryoState::previous_session_crashed`;
    /// an outer-loop `Err` from `run_one_session` is also a crash but is
    /// handled separately because there is no outcome to ask.
    fn is_crash(&self) -> bool {
        matches!(self, SessionLoopOutcome::ValidationFailed { .. })
    }
}

mod effects;
mod request;
mod session;

use effects::{ReplyAuthor, SessionEffects};

#[cfg(test)]
use request::TodoRequest;
use request::{
    handle_todo_request, resolve_hibernate_request, DaemonRequest, FileTodoEffects,
    TodoRequestOutcome,
};
#[cfg(test)]
use session::ChildExitStatus;
use session::{ProcessSessionLauncher, SessionLauncher, SessionRuntime};

/// Pure: given the next scheduled wake and (optionally) a session-registered
/// fallback action, produce the `(deadline, action)` to arm. We arm the
/// fallback one hour after the scheduled wake so a missed wake fires the
/// alert rather than silently dropping it.
fn scheduled_fallback_for(
    next_wake: Option<NaiveDateTime>,
    fallback: Option<FallbackAction>,
) -> Option<(NaiveDateTime, FallbackAction)> {
    next_wake.and_then(|w| fallback.map(|f| (w + chrono::Duration::hours(1), f)))
}

/// Pure: given the configured rotate-on policy and the provider pool, decide
/// whether a failed session should trigger provider rotation.
fn should_rotate_provider(
    rotate_on: &crate::config::RotateOn,
    quick_exit: bool,
    provider_count: usize,
) -> bool {
    if provider_count < 2 {
        return false;
    }
    match rotate_on {
        crate::config::RotateOn::QuickExit => quick_exit,
        crate::config::RotateOn::AnyFailure => true,
        crate::config::RotateOn::Never => false,
    }
}

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRunResult<'a> {
    Outcome(&'a SessionLoopOutcome),
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RetryPlan {
    backoff: Duration,
    send_alert: bool,
}

impl RetryPlan {
    fn for_state(retry: &RetryState) -> Self {
        Self {
            backoff: retry.next_backoff(),
            send_alert: retry.attempt.saturating_add(1) == retry.max_retries,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderRotationReason {
    QuickExit,
    Failure,
}

impl ProviderRotationReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::QuickExit => "quick-exit",
            Self::Failure => "failure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NextStep {
    PlanComplete,
    Hibernate {
        next_wake: Option<NaiveDateTime>,
        scheduled_fallback: Option<(NaiveDateTime, FallbackAction)>,
    },
    RotateProvider {
        next_wake: Option<NaiveDateTime>,
        next_provider_index: usize,
        wrapped: bool,
        reason: ProviderRotationReason,
    },
    Retry {
        next_wake: Option<NaiveDateTime>,
        plan: RetryPlan,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Break,
    Continue,
    Idle,
}

struct ActiveSessionContext<'a> {
    cryo_state: &'a CryoState,
    inbox_filenames: Vec<String>,
    timeout_secs: u64,
    spawn_time: Instant,
}

struct ActiveRequestState<'a> {
    logger: &'a mut crate::log::EventLogger,
    pending_fallback: &'a mut Option<FallbackAction>,
    hibernate_outcome: &'a mut Option<SessionLoopOutcome>,
    reply_tracker: &'a mut HumanReplyTracker,
}

struct EventLoopMutations<'a> {
    cryo_state: &'a mut CryoState,
    retry: &'a mut RetryState,
    pending_fallback: &'a mut Option<(NaiveDateTime, FallbackAction)>,
    next_wake: &'a mut Option<NaiveDateTime>,
    run_now: &'a mut bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonBootstrapState {
    next_report_time: Option<NaiveDateTime>,
    next_wake: Option<NaiveDateTime>,
    run_now: bool,
    pending_fallback: Option<(NaiveDateTime, FallbackAction)>,
    watch_inbox_path: Option<PathBuf>,
    cleared_invalid_pending_fallback: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HumanReplyTracker {
    known_filenames: Vec<String>,
    unreplied_filenames: Vec<String>,
}

impl HumanReplyTracker {
    fn new(initial_filenames: &[String]) -> Self {
        let mut tracker = Self::default();
        tracker.record_inbox_messages(initial_filenames);
        tracker
    }

    fn record_inbox_messages(&mut self, filenames: &[String]) {
        for filename in filenames {
            if self.known_filenames.iter().any(|known| known == filename) {
                continue;
            }
            self.known_filenames.push(filename.clone());
            self.unreplied_filenames.push(filename.clone());
        }
    }

    fn record_agent_reply(&mut self) {
        self.unreplied_filenames.clear();
    }

    fn known_filenames(&self) -> &[String] {
        &self.known_filenames
    }

    fn unreplied_filenames(&self) -> &[String] {
        &self.unreplied_filenames
    }
}

fn daemon_unanswered_reply_text(message_count: usize) -> String {
    let (noun, verb) = if message_count == 1 {
        ("message", "is")
    } else {
        ("messages", "are")
    };
    format!(
        "I received {message_count} {noun}, but the agent did not send a reply before the session ended. \
         The daemon is replying so your {noun} {verb} not left unanswered."
    )
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

    fn register_signal_handlers(
        &self,
        shutdown: &Arc<AtomicBool>,
        wake_requested: &Arc<AtomicBool>,
    ) -> Result<()>;
    fn bind_socket_server(&self, sock_path: &Path) -> Result<Self::Server>;
    fn register_registry(&self, dir: &Path, sock_path: &Path) -> Result<()>;
    fn start_inbox_watcher(
        &self,
        inbox_path: &Path,
        tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<Self::Watcher>;
}

struct SystemStartupPlatform;

impl StartupPlatform for SystemStartupPlatform {
    type Server = crate::socket::SocketServer;
    type Watcher = InboxWatcher;

    fn register_signal_handlers(
        &self,
        shutdown: &Arc<AtomicBool>,
        wake_requested: &Arc<AtomicBool>,
    ) -> Result<()> {
        flag::register(SIGTERM, Arc::clone(shutdown))
            .context("Failed to register SIGTERM handler")?;
        flag::register(SIGINT, Arc::clone(shutdown))
            .context("Failed to register SIGINT handler")?;
        flag::register(SIGUSR1, Arc::clone(wake_requested))
            .context("Failed to register SIGUSR1 handler")?;
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
        inbox_path: &Path,
        tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<Self::Watcher> {
        InboxWatcher::start(inbox_path, tx)
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
            outcome: SessionLoopOutcome::ValidationFailed { quick_exit: false },
            finish_reason: "daemon shutdown — agent terminated",
        },
        (SessionInterruption::Timeout, None) => InterruptedSessionDecision {
            outcome: SessionLoopOutcome::ValidationFailed { quick_exit: false },
            finish_reason: "session timeout — agent killed",
        },
    }
}

fn resolve_child_exit(
    hibernate_outcome: Option<SessionLoopOutcome>,
    elapsed: Duration,
) -> ChildExitDecision {
    if let Some(outcome) = hibernate_outcome {
        return ChildExitDecision {
            outcome,
            finish_reason: "session complete",
            quick_exit: false,
        };
    }

    let quick_exit = elapsed < Duration::from_secs(5);
    ChildExitDecision {
        outcome: SessionLoopOutcome::ValidationFailed { quick_exit },
        finish_reason: "agent exited without hibernate",
        quick_exit,
    }
}

fn decide_next_step(
    session_result: SessionRunResult<'_>,
    config: &CryoConfig,
    retry: &RetryState,
    next_wake: Option<NaiveDateTime>,
) -> NextStep {
    match session_result {
        SessionRunResult::Outcome(SessionLoopOutcome::PlanComplete) => NextStep::PlanComplete,
        SessionRunResult::Outcome(SessionLoopOutcome::Hibernate { fallback }) => {
            NextStep::Hibernate {
                next_wake,
                scheduled_fallback: scheduled_fallback_for(next_wake, fallback.clone()),
            }
        }
        SessionRunResult::Outcome(SessionLoopOutcome::ValidationFailed { quick_exit }) => {
            if should_rotate_provider(&config.rotate_on, *quick_exit, config.providers.len()) {
                let provider_count = config.providers.len();
                let next_provider_index = (retry.provider_index + 1) % provider_count;
                return NextStep::RotateProvider {
                    next_wake,
                    next_provider_index,
                    wrapped: next_provider_index == 0,
                    reason: if *quick_exit {
                        ProviderRotationReason::QuickExit
                    } else {
                        ProviderRotationReason::Failure
                    },
                };
            }
            NextStep::Retry {
                next_wake,
                plan: RetryPlan::for_state(retry),
            }
        }
        SessionRunResult::Error => NextStep::Retry {
            next_wake,
            plan: RetryPlan::for_state(retry),
        },
    }
}

/// Compute how long to sleep given optional wake and report deadlines.
fn compute_sleep_timeout(
    wake_deadline: Option<NaiveDateTime>,
    report_deadline: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> Duration {
    let to_duration =
        |dt: NaiveDateTime| -> Duration { (dt - now).to_std().unwrap_or(Duration::ZERO) };
    match (
        wake_deadline.map(&to_duration),
        report_deadline.map(&to_duration),
    ) {
        (Some(w), Some(r)) => w.min(r),
        (Some(w), None) => w,
        (None, Some(r)) => r,
        (None, None) => Duration::from_secs(3600),
    }
}

const PREVIOUS_SESSION_CRASH_NOTICE: &str =
    "PREVIOUS SESSION CRASHED: The agent exited without calling \
     `cryo-agent hibernate`. A reply may have been partially sent. \
     Check `messages/inbox/archive/` for any message that arrived \
     during the crashed session; if it still needs a user-visible \
     response, send it now via `cryo-agent reply` or \
     `cryo-agent send` before doing the normal session work.";

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

/// Compute the next wake time from the TODO list.
/// Iterates all pending TODOs, parses each `at` field, and returns the earliest
/// valid timestamp. Invalid or unparseable entries are skipped with a warning.
fn next_wake_from_todos(dir: &Path) -> Option<NaiveDateTime> {
    let path = dir.join("todo.json");
    let list = crate::todo::TodoList::load(&path).ok()?;
    list.items()
        .iter()
        .filter(|i| !i.done && !i.at.is_empty())
        .filter_map(|i| {
            let parsed = NaiveDateTime::parse_from_str(&i.at, WAKE_TIME_FMT);
            if parsed.is_err() {
                eprintln!(
                    "Daemon: Skipping TODO #{} with invalid at value: {:?}",
                    i.id, i.at
                );
            }
            parsed.ok()
        })
        .min()
}

/// Check if the scheduled wake time is significantly in the past (machine suspend).
/// Returns `Some(delay_description)` if delayed by more than 5 minutes.
fn detect_delayed_wake(scheduled: NaiveDateTime, now: NaiveDateTime) -> Option<String> {
    let delay = now - scheduled;
    if delay > chrono::Duration::minutes(5) {
        let delay_str = if delay.num_hours() > 0 {
            format!("{}h {}m", delay.num_hours(), delay.num_minutes() % 60)
        } else {
            format!("{}m", delay.num_minutes())
        };
        Some(delay_str)
    } else {
        None
    }
}

fn delayed_wake_notice(
    is_inbox_wake: bool,
    next_wake: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> Option<String> {
    match (is_inbox_wake, next_wake) {
        (true, _) | (_, None) => None,
        (false, Some(wake)) => detect_delayed_wake(wake, now).map(|delay_str| {
            format!(
                "DELAYED WAKE: This session was scheduled for {} but is running {} late \
                 (the host machine was likely suspended or powered off). \
                 Check whether time-sensitive tasks need adjustment.",
                wake.format(WAKE_TIME_FMT),
                delay_str,
            )
        }),
    }
}

fn pending_fallback_to_state(
    pending: Option<&(NaiveDateTime, FallbackAction)>,
) -> Option<PendingFallbackState> {
    pending.map(|(deadline, action)| PendingFallbackState {
        deadline: deadline.format(FALLBACK_TIME_FMT).to_string(),
        action: action.clone(),
    })
}

fn pending_fallback_from_state(
    state: &CryoState,
) -> Result<Option<(NaiveDateTime, FallbackAction)>> {
    let Some(pending) = state.pending_fallback.as_ref() else {
        return Ok(None);
    };
    let deadline = NaiveDateTime::parse_from_str(&pending.deadline, FALLBACK_TIME_FMT)
        .with_context(|| format!("Invalid pending fallback deadline: {}", pending.deadline))?;
    Ok(Some((deadline, pending.action.clone())))
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
    wake_requested: Arc<AtomicBool>,
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
            wake_requested: Arc::new(AtomicBool::new(false)),
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

    /// Test-only constructor: inject a custom `StateStore` to drive
    /// save-failure paths deterministically.
    #[cfg(test)]
    fn new_with_state_store(
        dir: PathBuf,
        clock: Arc<dyn Clock>,
        launcher: Arc<dyn SessionLauncher>,
        state_store: Arc<dyn StateStore>,
    ) -> Self {
        Self::with_deps(dir, clock, launcher, state_store)
    }

    /// All in-daemon state writes funnel through this, so tests can stub
    /// `StateStore` and all save paths respond consistently.
    fn save_state(&self, cryo_state: &CryoState) -> Result<()> {
        self.state_store.save(&self.state_path, cryo_state)
    }

    /// Atomic mutation of the *scheduled* fallback slot: set the value, keep
    /// `CryoState::pending_fallback` in sync, and persist. Errors are the
    /// caller's to handle (see per-call-site policy in the refactor plan).
    fn set_pending_fallback(
        &self,
        cryo_state: &mut CryoState,
        slot: &mut Option<(NaiveDateTime, FallbackAction)>,
        new: Option<(NaiveDateTime, FallbackAction)>,
    ) -> Result<()> {
        *slot = new;
        cryo_state.pending_fallback = pending_fallback_to_state(slot.as_ref());
        self.save_state(cryo_state)
    }

    fn sync_pending_fallback_state(
        &self,
        cryo_state: &mut CryoState,
        pending: Option<&(NaiveDateTime, FallbackAction)>,
    ) {
        cryo_state.pending_fallback = pending_fallback_to_state(pending);
    }

    fn build_bootstrap_state(
        &self,
        cryo_state: &mut CryoState,
        config: &CryoConfig,
    ) -> DaemonBootstrapState {
        let last_report = cryo_state
            .last_report_time
            .as_ref()
            .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok());
        let next_report_time = crate::report::compute_next_report_time(
            &config.report_time,
            config.report_interval,
            last_report,
        );

        let next_wake = next_wake_from_todos(&self.dir);
        let run_now = cryo_state.session_number == 0
            || next_wake.is_some_and(|w| self.clock.local_now() >= w);

        let inbox_path = self.dir.join("messages").join("inbox");
        let watch_inbox_path = if config.watch_inbox && inbox_path.exists() {
            Some(inbox_path)
        } else {
            None
        };

        let (pending_fallback, cleared_invalid_pending_fallback) =
            match pending_fallback_from_state(cryo_state) {
                Ok(pending) => (pending, false),
                Err(e) => {
                    eprintln!("Daemon: clearing invalid pending fallback state: {e}");
                    cryo_state.pending_fallback = None;
                    (None, true)
                }
            };

        DaemonBootstrapState {
            next_report_time,
            next_wake,
            run_now,
            pending_fallback,
            watch_inbox_path,
            cleared_invalid_pending_fallback,
        }
    }

    fn prepare_shutdown_state(
        &self,
        cryo_state: &mut CryoState,
        pending: Option<&(NaiveDateTime, FallbackAction)>,
    ) {
        cryo_state.pid = None;
        cryo_state.instance_id = None;
        self.sync_pending_fallback_state(cryo_state, pending);
    }

    fn prepare_runtime_startup<P: StartupPlatform>(
        &self,
        platform: &P,
        watch_inbox_path: Option<&Path>,
        tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<StartupResources<P::Server, P::Watcher>> {
        platform.register_signal_handlers(&self.shutdown, &self.wake_requested)?;

        let sock_path = crate::socket::socket_path(&self.dir);
        if let Some(parent) = sock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let server = platform.bind_socket_server(&sock_path)?;

        let registry_warning = platform
            .register_registry(&self.dir, &sock_path)
            .err()
            .map(|e| e.to_string());

        let (watcher, watcher_warning) = if let Some(inbox_path) = watch_inbox_path {
            match platform.start_inbox_watcher(inbox_path, tx) {
                Ok(watcher) => (Some(watcher), None),
                Err(e) => (None, Some(e.to_string())),
            }
        } else {
            (None, None)
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
            DaemonRequest::Todo(todo_request) => {
                let mut effects = FileTodoEffects::new(&self.dir);
                let response = handle_todo_request(todo_request, &mut effects).into_response();
                let _ = responder.respond(&response);
            }
            DaemonRequest::Hibernate { .. }
            | DaemonRequest::Alert { .. }
            | DaemonRequest::Reply { .. }
            | DaemonRequest::Receive => {
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
        config: &CryoConfig,
        state: EventLoopMutations<'_>,
    ) -> Result<LoopControl> {
        match step {
            NextStep::PlanComplete => {
                state.retry.reset();
                // Save-failure policy: log and still break. On restart, stale
                // on-disk fallback is harmless because the session log / plan
                // state shows the plan is done; the daemon won't sleep again.
                if let Err(e) =
                    self.set_pending_fallback(state.cryo_state, state.pending_fallback, None)
                {
                    eprintln!("Daemon: failed to persist cleared fallback on PlanComplete: {e}");
                }
                eprintln!("Daemon: plan complete. Shutting down.");
                Ok(LoopControl::Break)
            }
            NextStep::Hibernate {
                next_wake: refreshed_next_wake,
                scheduled_fallback,
            } => {
                state.retry.reset();
                *state.next_wake = refreshed_next_wake;
                // Save-failure policy: escalate to failure retry. If we can't
                // persist the armed fallback, do not sleep — a crash before the
                // next save would lose the fallback entirely.
                if let Err(e) = self.set_pending_fallback(
                    state.cryo_state,
                    state.pending_fallback,
                    scheduled_fallback,
                ) {
                    eprintln!(
                        "Daemon: failed to persist armed fallback after Hibernate: {e}. \
                         Escalating to failure-retry so the daemon does not sleep \
                         with an unpersisted fallback."
                    );
                    let plan = RetryPlan::for_state(state.retry);
                    if self.apply_failure_retry_plan(state.retry, plan, &config.fallback_alert) {
                        return Ok(LoopControl::Break);
                    }
                    *state.run_now = true;
                    return Ok(LoopControl::Continue);
                }
                if let Some(w) = *state.next_wake {
                    eprintln!("Daemon: next wake at {}", w.format("%Y-%m-%d %H:%M"));
                } else {
                    eprintln!("Daemon: no pending TODOs, idling");
                }
                Ok(LoopControl::Idle)
            }
            NextStep::RotateProvider {
                next_wake: refreshed_next_wake,
                next_provider_index,
                wrapped,
                reason,
            } => {
                *state.next_wake = refreshed_next_wake;
                let old_name = config
                    .providers
                    .get(state.retry.provider_index)
                    .map(|p| p.name.as_str())
                    .unwrap_or("unknown");
                state.retry.provider_index = next_provider_index;
                state.retry.attempt = 0;
                let new_name = config
                    .providers
                    .get(state.retry.provider_index)
                    .map(|p| p.name.as_str())
                    .unwrap_or("unknown");
                eprintln!(
                    "Daemon: rotating provider: {} -> {} (reason: {})",
                    old_name,
                    new_name,
                    reason.as_str(),
                );

                // Persist immediately so `cryo status` reflects the change.
                state.cryo_state.provider_index = Some(state.retry.provider_index);
                let _ = self.save_state(state.cryo_state);

                if wrapped {
                    // All providers tried — apply backoff before next cycle.
                    eprintln!("Daemon: all providers tried, backing off before next cycle");
                    if self.sleep_or_shutdown(Duration::from_secs(60)) {
                        return Ok(LoopControl::Break);
                    }
                }
                *state.run_now = true;
                Ok(LoopControl::Continue)
            }
            NextStep::Retry {
                next_wake: refreshed_next_wake,
                plan,
            } => {
                *state.next_wake = refreshed_next_wake;
                if self.apply_failure_retry_plan(state.retry, plan, &config.fallback_alert) {
                    return Ok(LoopControl::Break);
                }
                *state.run_now = true;
                Ok(LoopControl::Continue)
            }
        }
    }

    /// Run the daemon event loop. Blocks until SIGTERM or plan completion.
    pub fn run(&self) -> Result<()> {
        let mut cryo_state =
            state::load_state(&self.state_path)?.context("No cryochamber state found")?;

        // Guard: refuse to start if another daemon is already running
        if state::is_locked(&cryo_state) {
            anyhow::bail!(
                "Another daemon is already running (PID: {:?}). Use `cryo cancel` first.",
                cryo_state.pid
            );
        }

        // Load project config from cryo.toml (fall back to defaults for legacy projects)
        let mut config =
            crate::config::load_config(&crate::config::config_path(&self.dir))?.unwrap_or_default();
        config.apply_overrides(&cryo_state);
        let bootstrap = self.build_bootstrap_state(&mut cryo_state, &config);

        // Save PID so other commands can detect the running daemon
        cryo_state.pid = Some(std::process::id());
        cryo_state.instance_id = Some(state::new_instance_id());
        self.save_state(&cryo_state)?;

        let (tx, rx) = mpsc::channel();
        let startup = match self.prepare_runtime_startup(
            &SystemStartupPlatform,
            bootstrap.watch_inbox_path.as_deref(),
            tx.clone(),
        ) {
            Ok(startup) => startup,
            Err(e) => {
                self.prepare_shutdown_state(&mut cryo_state, bootstrap.pending_fallback.as_ref());
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
            eprintln!("Daemon: failed to register in ~/.cryo/daemons: {warning}");
        }

        let watcher_started = startup.watcher.is_some();
        match watcher_startup_notice(
            startup.diagnostics.watcher_warning.as_deref(),
            watcher_started,
        ) {
            WatcherStartupNotice::Warning(warning) => {
                eprintln!("Daemon: failed to start inbox watcher: {warning}");
            }
            WatcherStartupNotice::Started => {
                eprintln!("Daemon: watching messages/inbox/ for new messages");
            }
            WatcherStartupNotice::Silent => {}
        }
        let _watcher = startup.watcher;

        // Spawn a thread that forwards signals to the event channel,
        // so recv_timeout() unblocks immediately on SIGTERM/SIGINT/SIGUSR1.
        let shutdown_flag = Arc::clone(&self.shutdown);
        let wake_flag = Arc::clone(&self.wake_requested);
        let signal_tx = tx;
        let signal_clock = Arc::clone(&self.clock);
        std::thread::spawn(move || loop {
            signal_clock.sleep(Duration::from_millis(250));
            if shutdown_flag.load(Ordering::Relaxed) {
                let _ = signal_tx.send(DaemonEvent::Shutdown);
                break;
            }
            if wake_flag.swap(false, Ordering::Relaxed) {
                let _ = signal_tx.send(DaemonEvent::InboxChanged);
            }
        });

        // The event loop persists final state (pid, pending_fallback) before
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
        let mut next_report_time = bootstrap.next_report_time;
        if config.report_interval > 0 && next_report_time.is_none() {
            eprintln!(
                "Daemon: warning: report_interval={} but report_time='{}' is invalid (expected HH:MM)",
                config.report_interval, config.report_time
            );
        }
        if let Some(nrt) = next_report_time {
            eprintln!("Daemon: next report at {}", nrt.format("%Y-%m-%d %H:%M"));
        }

        let provider_count = config.providers.len();
        let mut retry = RetryState::new(config.max_retries, provider_count);
        let mut next_wake = bootstrap.next_wake;
        let mut run_now = bootstrap.run_now;
        let mut inbox_wake = false;
        let mut pending_fallback = bootstrap.pending_fallback;

        // Replay any fallback that was in-flight when the previous daemon
        // crashed. Runs exactly once before the main loop so the operator
        // hears about a dead-man alert even if the previous run died mid-fire.
        if let Err(e) = self.replay_in_flight_fallback(cryo_state, &config.fallback_alert) {
            eprintln!("Daemon: in-flight fallback replay failed: {e:#}");
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

                // Detect delayed wake: if the scheduled wake time has long passed
                // (e.g. computer was sleeping), notify the agent instead of failing.
                // Skip this check for inbox-triggered wakes — the agent should handle
                // the user's message without a spurious delay warning.
                let delayed_wake =
                    delayed_wake_notice(is_inbox_wake, next_wake, self.clock.local_now());
                if delayed_wake.is_some() && pending_fallback.is_some() {
                    // Delayed wake means we already slept past the deadline;
                    // the armed fallback is stale. Save-failure policy: log and
                    // keep running — the in-memory clear is authoritative, and
                    // the next successful save will converge disk.
                    if let Err(e) =
                        self.set_pending_fallback(cryo_state, &mut pending_fallback, None)
                    {
                        eprintln!(
                            "Daemon: failed to persist cleared fallback after delayed wake: {e}"
                        );
                    }
                }
                cryo_state.session_number += 1;
                if !config.providers.is_empty() {
                    cryo_state.provider_index = Some(retry.provider_index);
                }
                let _ = self.save_state(cryo_state);

                // Build provider env for this session
                let active_provider = config.providers.get(retry.provider_index);
                let provider_env: std::collections::HashMap<String, String> =
                    active_provider.map(|p| p.env.clone()).unwrap_or_default();
                let provider_name = active_provider.map(|p| p.name.as_str());

                let session_result = match self.run_one_session(
                    config,
                    cryo_state,
                    server,
                    delayed_wake.as_deref(),
                    &provider_env,
                    provider_name,
                ) {
                    Ok(outcome) => {
                        // Single source of truth: outcome decides crash-status.
                        cryo_state.previous_session_crashed = outcome.is_crash();
                        // Persist session number only after successful completion
                        self.save_state(cryo_state)?;
                        Ok(outcome)
                    }
                    Err(e) => {
                        cryo_state.session_number -= 1;
                        cryo_state.previous_session_crashed = true;
                        let _ = self.save_state(cryo_state);
                        eprintln!("Daemon: session failed: {e}");
                        Err(())
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
                let step =
                    decide_next_step(session_result_ref, config, &retry, refreshed_next_wake);
                match self.apply_next_step(
                    step,
                    config,
                    EventLoopMutations {
                        cryo_state,
                        retry: &mut retry,
                        pending_fallback: &mut pending_fallback,
                        next_wake: &mut next_wake,
                        run_now: &mut run_now,
                    },
                )? {
                    LoopControl::Break => break,
                    LoopControl::Continue => continue,
                    LoopControl::Idle => {}
                }
            }

            let expected_instance_id = cryo_state.instance_id.as_deref();
            self.service_idle_socket_requests(server, expected_instance_id);

            // Check fallback only when idle (not about to run a session).
            // Log errors prominently and keep running — a failed fallback is
            // visible to operators via the log; a crashed daemon would be
            // strictly worse than a missed alert.
            if let Err(e) =
                self.check_fallback(cryo_state, &mut pending_fallback, &config.fallback_alert)
            {
                eprintln!("Daemon: check_fallback failed: {e:#}");
            }

            // Check if periodic report is due
            if let Some(report_time) = next_report_time {
                if self.clock.local_now() >= report_time {
                    self.send_periodic_report(config, cryo_state, &mut next_report_time);
                }
            }

            // Wait for next event
            let timeout =
                compute_sleep_timeout(next_wake, next_report_time, self.clock.local_now())
                    .min(Duration::from_millis(250));

            match wait_for_idle_event(rx, timeout, next_wake, || self.clock.local_now()) {
                IdleWaitOutcome::WakeFromInbox => {
                    eprintln!("Daemon: inbox changed, waking up");
                    run_now = true;
                    inbox_wake = true;
                }
                IdleWaitOutcome::WakeFromSchedule => {
                    eprintln!("Daemon: scheduled wake time reached");
                    run_now = true;
                }
                IdleWaitOutcome::Shutdown => break,
                IdleWaitOutcome::StayIdle => {}
                IdleWaitOutcome::Disconnected => {
                    eprintln!("Daemon: event channel disconnected");
                    break;
                }
            }
        }

        // Persist pid=None and final pending_fallback so external observers
        // (e.g. the hub, `cryo status`) see a consistent shutdown state.
        self.prepare_shutdown_state(cryo_state, pending_fallback.as_ref());
        if let Err(e) = self.save_state(cryo_state) {
            eprintln!("Daemon: failed to save final state: {e}");
        }

        Ok(())
    }

    /// Delegate to the injected `SessionLauncher`.
    ///
    /// In production the launcher is `ProcessSessionLauncher`, which spawns a
    /// real agent. In tests a `ScriptedSessionLauncher` returns canned
    /// outcomes so the outer event loop can be exercised without wall-clock
    /// delays or subprocess management.
    fn run_one_session(
        &self,
        config: &CryoConfig,
        cryo_state: &CryoState,
        server: &crate::socket::SocketServer,
        delayed_wake: Option<&str>,
        provider_env: &std::collections::HashMap<String, String>,
        provider_name: Option<&str>,
    ) -> Result<SessionLoopOutcome> {
        self.launcher.run_session(
            self,
            config,
            cryo_state,
            server,
            delayed_wake,
            provider_env,
            provider_name,
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
                let _ = runtime.respond(true, "pong".into());
            }
            DaemonRequest::Hibernate {
                complete,
                exit_code,
                summary,
            } => {
                let decision = resolve_hibernate_request(
                    complete,
                    exit_code,
                    summary.as_deref(),
                    effects.has_pending_todo_with_valid_wake(),
                    state.pending_fallback.take(),
                );
                state.logger.log_event(&decision.log_event)?;
                *state.pending_fallback = decision.remaining_session_fallback;
                if let Some(outcome) = decision.outcome {
                    *state.hibernate_outcome = Some(outcome);
                }
                let _ = runtime.respond(decision.response_ok, decision.response_message.into());
            }
            DaemonRequest::Alert {
                action,
                target,
                message,
            } => {
                state
                    .logger
                    .log_event(&format!("alert: {action} -> {target}"))?;
                *state.pending_fallback = Some(FallbackAction {
                    action,
                    target,
                    message,
                });
                let _ = runtime.respond(true, "Alert registered".into());
            }
            DaemonRequest::Reply { text } => {
                match effects.write_reply(ReplyAuthor::Agent, &text, self.clock.local_now()) {
                    Ok(()) => {
                        state.reply_tracker.record_agent_reply();
                        state.logger.log_event(&format!("reply: \"{text}\""))?;
                        let _ = runtime.respond(true, "Reply sent".into());
                    }
                    Err(e) => {
                        state.logger.log_event(&format!("reply failed: {e}"))?;
                        let _ = runtime.respond(false, format!("Failed to write reply: {e}"));
                    }
                }
            }
            DaemonRequest::Todo(todo_request) => {
                let TodoRequestOutcome {
                    ok,
                    message,
                    log_event,
                } = handle_todo_request(todo_request, effects);
                if let Some(event) = log_event {
                    state.logger.log_event(&event)?;
                }
                let _ = runtime.respond(ok, message);
            }
            DaemonRequest::Receive => match effects.receive_inbox() {
                Ok((body, filenames)) => {
                    state.reply_tracker.record_inbox_messages(&filenames);
                    if filenames.is_empty() {
                        state.logger.log_event("receive: 0 messages")?;
                    } else {
                        state.logger.log_event(&format!(
                            "receive: {} message{} [{}]",
                            filenames.len(),
                            if filenames.len() == 1 { "" } else { "s" },
                            filenames.join(", "),
                        ))?;
                    }
                    let _ = runtime.respond(true, body);
                }
                Err(e) => {
                    state.logger.log_event(&format!("receive failed: {e}"))?;
                    let _ = runtime.respond(false, format!("Failed to receive: {e}"));
                }
            },
        }
        Ok(())
    }

    fn finalize_human_replies(
        &self,
        effects: &mut impl SessionEffects,
        logger: &mut crate::log::EventLogger,
        reply_tracker: &mut HumanReplyTracker,
    ) -> Result<()> {
        let unread = effects.list_inbox_filenames()?;
        reply_tracker.record_inbox_messages(&unread);

        let unreplied = reply_tracker.unreplied_filenames();
        if !unreplied.is_empty() {
            let text = daemon_unanswered_reply_text(unreplied.len());
            effects
                .write_reply(ReplyAuthor::Daemon, &text, self.clock.local_now())
                .context("failed to write daemon reply for unanswered inbox messages")?;
            logger.log_event(&format!(
                "daemon reply: {} unanswered inbox message{} [{}]",
                unreplied.len(),
                if unreplied.len() == 1 { "" } else { "s" },
                unreplied.join(", "),
            ))?;
        }

        if !reply_tracker.known_filenames().is_empty() {
            effects
                .archive_inbox_messages(reply_tracker.known_filenames())
                .context("failed to archive answered inbox messages")?;
        }

        Ok(())
    }

    fn drive_active_session(
        &self,
        runtime: &mut impl SessionRuntime,
        effects: &mut impl SessionEffects,
        context: ActiveSessionContext<'_>,
        mut logger: crate::log::EventLogger,
    ) -> Result<SessionLoopOutcome> {
        let deadline = if context.timeout_secs > 0 {
            Some(context.spawn_time + Duration::from_secs(context.timeout_secs))
        } else {
            None
        };

        let mut hibernate_outcome: Option<SessionLoopOutcome> = None;
        let mut pending_fallback: Option<FallbackAction> = None;
        let mut reply_tracker = HumanReplyTracker::new(&context.inbox_filenames);
        let expected_instance_id = context.cryo_state.instance_id.as_deref();

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                runtime.terminate();
                let decision = resolve_interrupted_session(
                    SessionInterruption::Shutdown,
                    hibernate_outcome.take(),
                );
                self.finalize_human_replies(effects, &mut logger, &mut reply_tracker)?;
                logger.finish(decision.finish_reason)?;
                return Ok(decision.outcome);
            }

            if let Some(d) = deadline {
                if self.clock.monotonic_now() >= d {
                    eprintln!(
                        "Daemon: session timeout ({}s) — killing agent",
                        context.timeout_secs
                    );
                    runtime.terminate();
                    let decision = resolve_interrupted_session(
                        SessionInterruption::Timeout,
                        hibernate_outcome.take(),
                    );
                    self.finalize_human_replies(effects, &mut logger, &mut reply_tracker)?;
                    logger.finish(decision.finish_reason)?;
                    return Ok(decision.outcome);
                }
            }

            match runtime.accept_request(expected_instance_id) {
                Ok(Some(request)) => self.handle_active_request(
                    request,
                    runtime,
                    effects,
                    ActiveRequestState {
                        logger: &mut logger,
                        pending_fallback: &mut pending_fallback,
                        hibernate_outcome: &mut hibernate_outcome,
                        reply_tracker: &mut reply_tracker,
                    },
                )?,
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
                    logger.log_event(&format!(
                        "agent exited (code {})",
                        status
                            .code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "signal".into())
                    ))?;

                    let decision = resolve_child_exit(hibernate_outcome.take(), elapsed);
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
                    self.finalize_human_replies(effects, &mut logger, &mut reply_tracker)?;
                    logger.finish(decision.finish_reason)?;
                    return Ok(decision.outcome);
                }
                Ok(None) => {}
                Err(e) => {
                    self.finalize_human_replies(effects, &mut logger, &mut reply_tracker)?;
                    logger.finish(&format!("error checking agent: {e}"))?;
                    return Err(e.into());
                }
            }

            if hibernate_outcome.is_some() {
                self.clock.sleep(Duration::from_millis(100));
                continue;
            }

            self.clock.sleep(Duration::from_millis(100));
        }
    }

    /// Execute a pending fallback if its deadline has passed.
    ///
    /// Returns `Ok(true)` if the fallback fired, `Ok(false)` if the deadline
    /// had not yet passed (or no fallback was armed), and `Err` if persisting
    /// the clear or executing the action failed. Errors are surfaced to the
    /// caller rather than swallowed so a misconfigured outbox or an unwritable
    /// state path does not silently consume a fallback.
    fn check_fallback(
        &self,
        cryo_state: &mut CryoState,
        pending: &mut Option<(NaiveDateTime, FallbackAction)>,
        alert_method: &str,
    ) -> Result<bool> {
        let due =
            matches!(pending.as_ref(), Some((deadline, _)) if self.clock.local_now() > *deadline);
        if !due {
            return Ok(false);
        }

        // Borrow-checker note: we've already established the slot is `Some`.
        // Use pattern matching instead of `take().unwrap()` so the branch is
        // total by construction.
        let fb = match pending.take() {
            Some((_, fb)) => fb,
            None => return Ok(false),
        };

        // `set_pending_fallback` with `None` keeps the CryoState field in
        // sync and persists — so the same atomic mutation contract applies to
        // check_fallback as to every other fallback-slot mutation in the
        // daemon.
        self.set_pending_fallback(cryo_state, pending, None)
            .context("failed to persist cleared pending fallback")?;

        // Record that we're about to fire *before* calling execute. If we
        // crash between this save and `clear_in_flight_fallback` below, the
        // next daemon startup sees the record and replays the alert with a
        // "(replay after crash)" prefix — losing a dead-man alert is strictly
        // worse than delivering it twice with a label.
        let deadline_str = self.clock.local_now().format(FALLBACK_TIME_FMT).to_string();
        let started_at = Local::now()
            .naive_local()
            .format(FALLBACK_TIME_FMT)
            .to_string();
        cryo_state.in_flight_fallback = Some(InFlightFallback {
            deadline: deadline_str,
            action: fb.clone(),
            started_at,
        });
        self.save_state(cryo_state)
            .context("failed to persist in-flight fallback marker")?;

        eprintln!("Daemon: fallback deadline passed, executing fallback action");
        let execute_result = fb.execute(&self.dir, alert_method);

        // Clear the in-flight record regardless of whether execute succeeded
        // or failed — the marker is about crash-safety across `execute`, not
        // about retrying a failed send. If execute returned Err, the caller
        // logs it; we don't keep firing on every tick.
        cryo_state.in_flight_fallback = None;
        if let Err(save_err) = self.save_state(cryo_state) {
            eprintln!(
                "Daemon: failed to clear in-flight fallback marker after execute: {save_err}"
            );
        }

        execute_result.context("fallback execution failed")?;
        Ok(true)
    }

    /// Replay a fallback alert that was in-flight when the daemon crashed.
    /// Called once at startup. On success, clears the marker so subsequent
    /// restarts don't keep replaying.
    ///
    /// Policy: delivery beats silence. We prepend "(replay after crash)" to
    /// the message so operators can tell this alert survived a restart, but
    /// we don't try to dedup against the original send — the original may
    /// have never left the daemon.
    fn replay_in_flight_fallback(
        &self,
        cryo_state: &mut CryoState,
        alert_method: &str,
    ) -> Result<bool> {
        let Some(record) = cryo_state.in_flight_fallback.take() else {
            return Ok(false);
        };
        eprintln!(
            "Daemon: replaying in-flight fallback from previous run (started at {})",
            record.started_at
        );
        // Persist the cleared marker first. If replay itself crashes, we
        // don't want to replay again on next start — that risks an infinite
        // replay loop on a fallback that deterministically crashes execute.
        self.save_state(cryo_state)
            .context("failed to persist cleared in-flight fallback marker before replay")?;

        let replay = FallbackAction {
            action: record.action.action.clone(),
            target: record.action.target.clone(),
            message: format!("(replay after crash) {}", record.action.message),
        };
        replay
            .execute(&self.dir, alert_method)
            .context("replayed fallback execution failed")?;
        Ok(true)
    }

    /// Apply a precomputed retry plan with exponential backoff (5s, 10s, ..., 1h cap).
    /// Sends an alert once when max_retries is reached, then keeps retrying at 1h.
    /// Returns true if the daemon should shut down.
    fn apply_failure_retry_plan(
        &self,
        retry: &mut RetryState,
        plan: RetryPlan,
        alert_method: &str,
    ) -> bool {
        retry.record_failure();
        // Send alert once when we first hit max_retries.
        if plan.send_alert {
            eprintln!(
                "Daemon: {} retries failed, sending alert. Will keep retrying.",
                retry.max_retries
            );
            self.send_retry_alert(alert_method);
        }
        eprintln!(
            "Daemon: retry {} in {}s",
            retry.attempt,
            plan.backoff.as_secs()
        );
        self.sleep_or_shutdown(plan.backoff)
    }

    /// Send a system alert when retries are exhausted.
    fn send_retry_alert(&self, alert_method: &str) {
        let fb = FallbackAction {
            action: "retry_exhausted".to_string(),
            target: "operator".to_string(),
            message: format!(
                "Agent failed to hibernate after multiple attempts. Daemon will keep retrying. Directory: {}",
                self.dir.display()
            ),
        };
        if let Err(e) = fb.execute(&self.dir, alert_method) {
            eprintln!("Daemon: retry alert failed: {e}");
        }
    }

    fn get_task(&self) -> Option<String> {
        crate::log::parse_latest_session_task(&self.log_path)
            .ok()
            .flatten()
    }

    /// Generate and send the periodic activity report.
    fn send_periodic_report(
        &self,
        config: &CryoConfig,
        cryo_state: &mut CryoState,
        next_report_time: &mut Option<NaiveDateTime>,
    ) {
        let since =
            chrono::Utc::now().naive_utc() - chrono::Duration::hours(config.report_interval as i64);
        match crate::report::generate_report(&self.log_path, since) {
            Ok(summary) => {
                let project_name = self
                    .dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                if let Err(e) =
                    crate::report::write_report_to_outbox(&self.dir, &summary, project_name)
                {
                    eprintln!("Daemon: report outbox write failed: {e}");
                }
                eprintln!(
                    "Daemon: report sent ({} sessions, {} failed)",
                    summary.total_sessions, summary.failed_sessions
                );
            }
            Err(e) => {
                eprintln!("Daemon: report generation failed: {e}");
            }
        }

        // Update state and advance timer
        let now = self.clock.local_now();
        let previous_last_report_time = cryo_state.last_report_time.clone();
        cryo_state.last_report_time = Some(now.format("%Y-%m-%dT%H:%M:%S").to_string());
        if let Err(e) = self.save_state(cryo_state) {
            eprintln!("Daemon: failed to persist last_report_time: {e}");
            cryo_state.last_report_time = previous_last_report_time;
            return;
        }
        *next_report_time = crate::report::compute_next_report_time(
            &config.report_time,
            config.report_interval,
            Some(now),
        );
        if let Some(next) = next_report_time {
            eprintln!("Daemon: next report at {}", next.format("%Y-%m-%d %H:%M"));
        }
    }

    /// Sleep for `duration`, but return early if shutdown is signaled.
    /// Returns true if shutdown was requested.
    fn sleep_or_shutdown(&self, duration: Duration) -> bool {
        let step = Duration::from_millis(250);
        let mut remaining = duration;
        while remaining > Duration::ZERO {
            if self.shutdown.load(Ordering::Relaxed) {
                return true;
            }
            let sleep_time = remaining.min(step);
            self.clock.sleep(sleep_time);
            remaining = remaining.saturating_sub(sleep_time);
        }
        false
    }
}

#[cfg(test)]
#[path = "unit_tests/daemon.rs"]
mod tests;

#[cfg(test)]
#[path = "unit_tests/daemon_properties.rs"]
mod property_tests;
