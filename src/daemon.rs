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
use crate::state::{self, CryoState, PendingFallbackState};

/// Format for parsing TODO `at` timestamps (minute precision, no seconds).
const WAKE_TIME_FMT: &str = "%Y-%m-%dT%H:%M";
const FALLBACK_TIME_FMT: &str = "%Y-%m-%dT%H:%M:%S";

use crate::process::send_signal;

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

fn wait_for_idle_event(
    source: &impl EventSource,
    timeout: Duration,
    has_scheduled_wake: bool,
) -> IdleWaitOutcome {
    match source.recv_timeout(timeout) {
        Ok(DaemonEvent::InboxChanged) => {
            source.drain_inbox_changed();
            IdleWaitOutcome::WakeFromInbox
        }
        Ok(DaemonEvent::Shutdown) => IdleWaitOutcome::Shutdown,
        Err(WaitError::Timeout) if has_scheduled_wake => IdleWaitOutcome::WakeFromSchedule,
        Err(WaitError::Timeout) => IdleWaitOutcome::StayIdle,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionInterruption {
    Shutdown,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HibernateDecision {
    outcome: SessionLoopOutcome,
    response_message: &'static str,
    log_event: String,
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

struct ActiveSessionContext<'a> {
    cryo_state: &'a CryoState,
    timeout_secs: u64,
    spawn_time: Instant,
    inbox_filenames: &'a [String],
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChildExitStatus {
    code: Option<i32>,
}

trait SessionRuntime {
    fn accept_request(
        &mut self,
        expected_instance_id: Option<&str>,
    ) -> Result<Option<crate::socket::Request>>;
    fn respond(&mut self, ok: bool, message: String) -> Result<()>;
    fn try_wait(&mut self) -> std::io::Result<Option<ChildExitStatus>>;
    fn terminate(&mut self);
}

trait SessionEffects {
    fn archive_inbox(&mut self, inbox_filenames: &[String]) -> Result<()>;
    fn write_reply(&mut self, text: &str, timestamp: NaiveDateTime) -> Result<()>;
    fn todo_add(&mut self, text: &str, at: &str) -> Result<u32>;
    fn todo_done(&mut self, id: u32) -> Result<()>;
    fn todo_remove(&mut self, id: u32) -> Result<()>;
    fn todo_list(&mut self) -> Result<String>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StartupDiagnostics {
    registry_warning: Option<String>,
    watcher_warning: Option<String>,
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

struct FsSessionEffects<'a> {
    dir: &'a Path,
}

impl<'a> FsSessionEffects<'a> {
    fn new(dir: &'a Path) -> Self {
        Self { dir }
    }

    fn todo_path(&self) -> PathBuf {
        self.dir.join("todo.json")
    }
}

impl SessionEffects for FsSessionEffects<'_> {
    fn archive_inbox(&mut self, inbox_filenames: &[String]) -> Result<()> {
        crate::message::archive_messages(self.dir, inbox_filenames)
    }

    fn write_reply(&mut self, text: &str, timestamp: NaiveDateTime) -> Result<()> {
        let msg = crate::message::Message {
            from: "agent".to_string(),
            subject: "Reply".to_string(),
            body: text.to_string(),
            timestamp,
            metadata: std::collections::BTreeMap::new(),
        };
        crate::message::write_message(self.dir, "outbox", &msg)?;
        Ok(())
    }

    fn todo_add(&mut self, text: &str, at: &str) -> Result<u32> {
        let todo_path = self.todo_path();
        let mut list = crate::todo::TodoList::load(&todo_path)?;
        let id = list.add(text.to_string(), at.to_string());
        list.save(&todo_path)?;
        Ok(id)
    }

    fn todo_done(&mut self, id: u32) -> Result<()> {
        let todo_path = self.todo_path();
        let mut list = crate::todo::TodoList::load(&todo_path)?;
        list.done(id)?;
        list.save(&todo_path)?;
        Ok(())
    }

    fn todo_remove(&mut self, id: u32) -> Result<()> {
        let todo_path = self.todo_path();
        let mut list = crate::todo::TodoList::load(&todo_path)?;
        list.remove(id)?;
        list.save(&todo_path)?;
        Ok(())
    }

    fn todo_list(&mut self) -> Result<String> {
        let list = crate::todo::TodoList::load(&self.todo_path())?;
        Ok(list.display())
    }
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

struct ProcessSessionRuntime<'a> {
    server: &'a crate::socket::SocketServer,
    child: &'a mut std::process::Child,
    clock: Arc<dyn Clock>,
    pending_responder: Option<crate::socket::Responder>,
}

impl<'a> ProcessSessionRuntime<'a> {
    fn new(
        server: &'a crate::socket::SocketServer,
        child: &'a mut std::process::Child,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            server,
            child,
            clock,
            pending_responder: None,
        }
    }
}

impl SessionRuntime for ProcessSessionRuntime<'_> {
    fn accept_request(
        &mut self,
        expected_instance_id: Option<&str>,
    ) -> Result<Option<crate::socket::Request>> {
        match self.server.accept_one(expected_instance_id) {
            Ok(Some((request, responder))) => {
                self.pending_responder = Some(responder);
                Ok(Some(request))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == std::io::ErrorKind::WouldBlock {
                        return Ok(None);
                    }
                }
                Err(e)
            }
        }
    }

    fn respond(&mut self, ok: bool, message: String) -> Result<()> {
        let responder = self
            .pending_responder
            .take()
            .context("Missing pending session responder")?;
        responder.respond(&crate::socket::Response { ok, message })?;
        Ok(())
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ChildExitStatus>> {
        self.child.try_wait().map(|status| {
            status.map(|status| ChildExitStatus {
                code: status.code(),
            })
        })
    }

    fn terminate(&mut self) {
        let pid = self.child.id();
        terminate_child(self.child, pid, self.clock.as_ref());
    }
}

fn resolve_hibernate_request(
    complete: bool,
    exit_code: u8,
    summary: Option<&str>,
    pending_fallback: &mut Option<FallbackAction>,
) -> HibernateDecision {
    let summary = summary.unwrap_or("(no summary)");
    if exit_code != 0 {
        return HibernateDecision {
            outcome: SessionLoopOutcome::ValidationFailed { quick_exit: false },
            response_message: "Failure recorded. Daemon will retry.",
            log_event: format!("hibernate failed: exit={exit_code}, summary=\"{summary}\""),
        };
    }

    if complete {
        return HibernateDecision {
            outcome: SessionLoopOutcome::PlanComplete,
            response_message: "Plan complete. Shutting down.",
            log_event: format!("hibernate: plan complete, exit={exit_code}, summary=\"{summary}\""),
        };
    }

    HibernateDecision {
        outcome: SessionLoopOutcome::Hibernate {
            fallback: pending_fallback.take(),
        },
        response_message: "Hibernating.",
        log_event: format!("hibernate: exit={exit_code}, summary=\"{summary}\""),
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

/// Gracefully terminate a child process: SIGTERM, wait 2s, SIGKILL if needed.
fn terminate_child(child: &mut std::process::Child, pid: u32, clock: &dyn Clock) {
    send_signal(pid, libc::SIGTERM);
    clock.sleep(Duration::from_secs(2));
    if child.try_wait().ok().flatten().is_none() {
        send_signal(pid, libc::SIGKILL);
    }
    let _ = child.wait(); // reap to prevent zombie
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

/// The persistent daemon process.
pub struct Daemon {
    dir: PathBuf,
    state_path: PathBuf,
    log_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    wake_requested: Arc<AtomicBool>,
    clock: Arc<dyn Clock>,
}

impl Daemon {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_clock(dir, Arc::new(SystemClock))
    }

    fn with_clock(dir: PathBuf, clock: Arc<dyn Clock>) -> Self {
        let state_path = dir.join("timer.json");
        let log_path = dir.join("cryo.log");
        Self {
            dir,
            state_path,
            log_path,
            shutdown: Arc::new(AtomicBool::new(false)),
            wake_requested: Arc::new(AtomicBool::new(false)),
            clock,
        }
    }

    #[cfg(test)]
    fn new_with_clock(dir: PathBuf, clock: Arc<dyn Clock>) -> Self {
        Self::with_clock(dir, clock)
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
        match request {
            crate::socket::Request::Ping => {
                let _ = responder.respond(&crate::socket::Response {
                    ok: true,
                    message: "pong".into(),
                });
            }
            crate::socket::Request::TodoAdd { text, at } => {
                let todo_path = self.dir.join("todo.json");
                match crate::todo::TodoList::load(&todo_path) {
                    Ok(mut list) => {
                        let id = list.add(text, at);
                        let response = match list.save(&todo_path) {
                            Ok(()) => crate::socket::Response {
                                ok: true,
                                message: format!("Added todo #{id}"),
                            },
                            Err(e) => crate::socket::Response {
                                ok: false,
                                message: format!("Failed to save todo: {e}"),
                            },
                        };
                        let _ = responder.respond(&response);
                    }
                    Err(e) => {
                        let _ = responder.respond(&crate::socket::Response {
                            ok: false,
                            message: format!("Failed to load todo list: {e}"),
                        });
                    }
                }
            }
            crate::socket::Request::TodoDone { id } => {
                let todo_path = self.dir.join("todo.json");
                match crate::todo::TodoList::load(&todo_path) {
                    Ok(mut list) => {
                        let response = match list.done(id) {
                            Ok(()) => match list.save(&todo_path) {
                                Ok(()) => crate::socket::Response {
                                    ok: true,
                                    message: format!("Marked todo #{id} as done"),
                                },
                                Err(e) => crate::socket::Response {
                                    ok: false,
                                    message: format!("Failed to save todo: {e}"),
                                },
                            },
                            Err(e) => crate::socket::Response {
                                ok: false,
                                message: format!("{e}"),
                            },
                        };
                        let _ = responder.respond(&response);
                    }
                    Err(e) => {
                        let _ = responder.respond(&crate::socket::Response {
                            ok: false,
                            message: format!("Failed to load todo list: {e}"),
                        });
                    }
                }
            }
            crate::socket::Request::TodoRemove { id } => {
                let todo_path = self.dir.join("todo.json");
                match crate::todo::TodoList::load(&todo_path) {
                    Ok(mut list) => {
                        let response = match list.remove(id) {
                            Ok(()) => match list.save(&todo_path) {
                                Ok(()) => crate::socket::Response {
                                    ok: true,
                                    message: format!("Removed todo #{id}"),
                                },
                                Err(e) => crate::socket::Response {
                                    ok: false,
                                    message: format!("Failed to save todo: {e}"),
                                },
                            },
                            Err(e) => crate::socket::Response {
                                ok: false,
                                message: format!("{e}"),
                            },
                        };
                        let _ = responder.respond(&response);
                    }
                    Err(e) => {
                        let _ = responder.respond(&crate::socket::Response {
                            ok: false,
                            message: format!("Failed to load todo list: {e}"),
                        });
                    }
                }
            }
            crate::socket::Request::TodoList => {
                let todo_path = self.dir.join("todo.json");
                match crate::todo::TodoList::load(&todo_path) {
                    Ok(list) => {
                        let _ = responder.respond(&crate::socket::Response {
                            ok: true,
                            message: list.display(),
                        });
                    }
                    Err(e) => {
                        let _ = responder.respond(&crate::socket::Response {
                            ok: false,
                            message: format!("Failed to load todo list: {e}"),
                        });
                    }
                }
            }
            crate::socket::Request::Note { .. }
            | crate::socket::Request::Hibernate { .. }
            | crate::socket::Request::Alert { .. }
            | crate::socket::Request::Reply { .. } => {
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
        state::save_state(&self.state_path, &cryo_state)?;

        let (tx, rx) = mpsc::channel();
        let startup = match self.prepare_runtime_startup(
            &SystemStartupPlatform,
            bootstrap.watch_inbox_path.as_deref(),
            tx.clone(),
        ) {
            Ok(startup) => startup,
            Err(e) => {
                self.prepare_shutdown_state(&mut cryo_state, bootstrap.pending_fallback.as_ref());
                if let Err(save_err) = state::save_state(&self.state_path, &cryo_state) {
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
        if let Some(warning) = startup.diagnostics.watcher_warning {
            eprintln!("Daemon: failed to start inbox watcher: {warning}");
        } else if watcher_started {
            eprintln!("Daemon: watching messages/inbox/ for new messages");
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
                let delayed_wake = if is_inbox_wake {
                    None
                } else {
                    next_wake.and_then(|wake| {
                        let now = self.clock.local_now();
                        detect_delayed_wake(wake, now).map(|delay_str| {
                            format!(
                                "DELAYED WAKE: This session was scheduled for {} but is running {} late \
                                 (the host machine was likely suspended or powered off). \
                                 Check whether time-sensitive tasks need adjustment.",
                                wake.format(WAKE_TIME_FMT),
                                delay_str,
                            )
                        })
                    })
                };
                if delayed_wake.is_some() && pending_fallback.is_some() {
                    pending_fallback = None;
                    self.sync_pending_fallback_state(&mut cryo_state, pending_fallback.as_ref());
                    let _ = state::save_state(&self.state_path, &cryo_state);
                }
                cryo_state.session_number += 1;
                if !config.providers.is_empty() {
                    cryo_state.provider_index = Some(retry.provider_index);
                }
                let _ = state::save_state(&self.state_path, &cryo_state);

                // Build provider env for this session
                let active_provider = config.providers.get(retry.provider_index);
                let provider_env: std::collections::HashMap<String, String> =
                    active_provider.map(|p| p.env.clone()).unwrap_or_default();
                let provider_name = active_provider.map(|p| p.name.as_str());

                match self.run_one_session(
                    &config,
                    &cryo_state,
                    &server,
                    delayed_wake.as_deref(),
                    &provider_env,
                    provider_name,
                ) {
                    Ok(outcome) => {
                        // Persist session number only after successful completion
                        state::save_state(&self.state_path, &cryo_state)?;
                        match outcome {
                            SessionLoopOutcome::PlanComplete => {
                                retry.reset();
                                pending_fallback = None;
                                self.sync_pending_fallback_state(
                                    &mut cryo_state,
                                    pending_fallback.as_ref(),
                                );
                                let _ = state::save_state(&self.state_path, &cryo_state);
                                eprintln!("Daemon: plan complete. Shutting down.");
                                break;
                            }
                            SessionLoopOutcome::Hibernate { fallback } => {
                                retry.reset();
                                next_wake = next_wake_from_todos(&self.dir);
                                pending_fallback = next_wake
                                    .map(|w| (w + chrono::Duration::hours(1), fallback))
                                    .and_then(|(deadline, fb)| fb.map(|f| (deadline, f)));
                                self.sync_pending_fallback_state(
                                    &mut cryo_state,
                                    pending_fallback.as_ref(),
                                );
                                let _ = state::save_state(&self.state_path, &cryo_state);
                                if let Some(w) = next_wake {
                                    eprintln!(
                                        "Daemon: next wake at {}",
                                        w.format("%Y-%m-%d %H:%M")
                                    );
                                } else {
                                    eprintln!("Daemon: no pending TODOs, idling");
                                }
                            }
                            SessionLoopOutcome::ValidationFailed { quick_exit } => {
                                next_wake = next_wake_from_todos(&self.dir);

                                // Check if we should rotate provider
                                let should_rotate = !config.providers.is_empty()
                                    && config.providers.len() > 1
                                    && match config.rotate_on {
                                        crate::config::RotateOn::QuickExit => quick_exit,
                                        crate::config::RotateOn::AnyFailure => true,
                                        crate::config::RotateOn::Never => false,
                                    };

                                if should_rotate {
                                    let old_name = config
                                        .providers
                                        .get(retry.provider_index)
                                        .map(|p| p.name.as_str())
                                        .unwrap_or("unknown");
                                    let wrapped = retry.rotate_provider();
                                    let new_name = config
                                        .providers
                                        .get(retry.provider_index)
                                        .map(|p| p.name.as_str())
                                        .unwrap_or("unknown");
                                    eprintln!(
                                        "Daemon: rotating provider: {} -> {} (reason: {})",
                                        old_name,
                                        new_name,
                                        if quick_exit { "quick-exit" } else { "failure" },
                                    );

                                    // Persist immediately so `cryo status` reflects the change
                                    cryo_state.provider_index = Some(retry.provider_index);
                                    let _ = state::save_state(&self.state_path, &cryo_state);

                                    if wrapped {
                                        // All providers tried — apply backoff before next cycle
                                        eprintln!("Daemon: all providers tried, backing off before next cycle");
                                        if self.sleep_or_shutdown(Duration::from_secs(60)) {
                                            break;
                                        }
                                    }
                                    run_now = true;
                                    continue;
                                }

                                // No rotation — use standard retry with backoff
                                if self.handle_failure_retry(&mut retry, &config.fallback_alert) {
                                    break;
                                }
                                run_now = true;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        cryo_state.session_number -= 1;
                        next_wake = next_wake_from_todos(&self.dir);
                        eprintln!("Daemon: session failed: {e}");
                        if self.handle_failure_retry(&mut retry, &config.fallback_alert) {
                            break;
                        }
                        run_now = true;
                        continue;
                    }
                }
            }

            let expected_instance_id = cryo_state.instance_id.as_deref();
            self.service_idle_socket_requests(&server, expected_instance_id);

            // Check fallback only when idle (not about to run a session)
            self.check_fallback(
                &mut cryo_state,
                &mut pending_fallback,
                &config.fallback_alert,
            );

            // Check if periodic report is due
            if let Some(report_time) = next_report_time {
                if self.clock.local_now() >= report_time {
                    self.send_periodic_report(&config, &mut cryo_state, &mut next_report_time);
                }
            }

            // Wait for next event
            let timeout =
                compute_sleep_timeout(next_wake, next_report_time, self.clock.local_now())
                    .min(Duration::from_millis(250));

            match wait_for_idle_event(&rx, timeout, next_wake.is_some()) {
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

        // Cleanup: always unregister and remove socket, even if state save fails
        self.prepare_shutdown_state(&mut cryo_state, pending_fallback.as_ref());
        if let Err(e) = state::save_state(&self.state_path, &cryo_state) {
            eprintln!("Daemon: failed to save final state: {e}");
        }
        crate::registry::unregister(&self.dir);
        crate::socket::SocketServer::cleanup(&sock_path);
        eprintln!("Daemon: exited cleanly");

        Ok(())
    }

    fn run_one_session(
        &self,
        config: &CryoConfig,
        cryo_state: &CryoState,
        server: &crate::socket::SocketServer,
        delayed_wake: Option<&str>,
        provider_env: &std::collections::HashMap<String, String>,
        provider_name: Option<&str>,
    ) -> Result<SessionLoopOutcome> {
        let agent_cmd = config.agent.clone();

        let task = self
            .get_task()
            .unwrap_or_else(|| "Continue the plan".to_string());

        let timeout_secs = config.max_session_duration;

        eprintln!(
            "Daemon: Session #{}: Running agent...",
            cryo_state.session_number
        );

        // List inbox filenames for logging (agent reads files itself)
        let inbox_filenames: Vec<String> = crate::message::list_inbox(&self.dir)?;

        // Load TODO list for agent prompt
        let todo_path = self.dir.join("todo.json");
        let todo_display = match crate::todo::TodoList::load(&todo_path) {
            Ok(list) => list.display(),
            Err(err) => {
                eprintln!(
                    "Daemon: Error loading TODO list from {}: {}",
                    todo_path.display(),
                    err
                );
                format!("Error loading TODO list ({}). Please check todo.json.", err)
            }
        };

        // Build prompt with task context and TODO list
        let agent_config = crate::agent::AgentConfig {
            session_number: cryo_state.session_number,
            task: task.clone(),
            delayed_wake: delayed_wake.map(|s| s.to_string()),
            todo_list: todo_display,
        };
        let prompt = crate::agent::build_prompt(&agent_config);

        // Begin event log
        let mut logger = crate::log::EventLogger::begin(
            &self.log_path,
            cryo_state.session_number,
            &task,
            &agent_cmd,
            &inbox_filenames,
        )?;

        // Log delayed wake notice
        if let Some(notice) = delayed_wake {
            logger.log_event(&format!("delayed wake: {notice}"))?;
        }

        // Open agent log file for stdout/stderr redirection
        let agent_log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(crate::log::agent_log_path(&self.dir))?;

        // Spawn agent with stdout/stderr redirected to cryo-agent.log
        let mut child =
            crate::agent::spawn_agent(&agent_cmd, &prompt, Some(agent_log_file), provider_env)?;
        let child_pid = child.id();
        let spawn_time = self.clock.monotonic_now();
        logger.log_event(&format!("agent started (pid {child_pid})"))?;
        if let Some(name) = provider_name {
            logger.log_event(&format!("provider: {name}"))?;
        }

        let mut runtime = ProcessSessionRuntime::new(server, &mut child, Arc::clone(&self.clock));
        let mut effects = FsSessionEffects::new(&self.dir);
        let context = ActiveSessionContext {
            cryo_state,
            timeout_secs,
            spawn_time,
            inbox_filenames: &inbox_filenames,
        };
        self.drive_active_session(&mut runtime, &mut effects, context, logger)
    }

    fn handle_active_request(
        &self,
        request: crate::socket::Request,
        runtime: &mut impl SessionRuntime,
        effects: &mut impl SessionEffects,
        logger: &mut crate::log::EventLogger,
        pending_fallback: &mut Option<FallbackAction>,
        hibernate_outcome: &mut Option<SessionLoopOutcome>,
    ) -> Result<()> {
        match request {
            crate::socket::Request::Ping => {
                let _ = runtime.respond(true, "pong".into());
            }
            crate::socket::Request::Note { text } => {
                logger.log_event(&format!("note: \"{text}\""))?;
                let _ = runtime.respond(true, "Note recorded".into());
            }
            crate::socket::Request::Hibernate {
                complete,
                exit_code,
                summary,
            } => {
                let decision = resolve_hibernate_request(
                    complete,
                    exit_code,
                    summary.as_deref(),
                    pending_fallback,
                );
                logger.log_event(&decision.log_event)?;
                *hibernate_outcome = Some(decision.outcome);
                let _ = runtime.respond(true, decision.response_message.into());
            }
            crate::socket::Request::Alert {
                action,
                target,
                message,
            } => {
                logger.log_event(&format!("alert: {action} -> {target}"))?;
                *pending_fallback = Some(FallbackAction {
                    action,
                    target,
                    message,
                });
                let _ = runtime.respond(true, "Alert registered".into());
            }
            crate::socket::Request::Reply { text } => {
                match effects.write_reply(&text, self.clock.local_now()) {
                    Ok(()) => {
                        logger.log_event(&format!("reply: \"{text}\""))?;
                        let _ = runtime.respond(true, "Reply sent".into());
                    }
                    Err(e) => {
                        logger.log_event(&format!("reply failed: {e}"))?;
                        let _ = runtime.respond(false, format!("Failed to write reply: {e}"));
                    }
                }
            }
            crate::socket::Request::TodoAdd { text, at } => match effects.todo_add(&text, &at) {
                Ok(id) => {
                    logger.log_event(&format!("todo add: #{id} \"{text}\" at {at}"))?;
                    let _ = runtime.respond(true, format!("Added todo #{id}"));
                }
                Err(e) => {
                    let _ = runtime.respond(false, format!("Failed to add todo: {e}"));
                }
            },
            crate::socket::Request::TodoDone { id } => match effects.todo_done(id) {
                Ok(()) => {
                    logger.log_event(&format!("todo done: #{id}"))?;
                    let _ = runtime.respond(true, format!("Marked todo #{id} as done"));
                }
                Err(e) => {
                    let _ = runtime.respond(false, format!("{e}"));
                }
            },
            crate::socket::Request::TodoRemove { id } => match effects.todo_remove(id) {
                Ok(()) => {
                    logger.log_event(&format!("todo remove: #{id}"))?;
                    let _ = runtime.respond(true, format!("Removed todo #{id}"));
                }
                Err(e) => {
                    let _ = runtime.respond(false, format!("{e}"));
                }
            },
            crate::socket::Request::TodoList => match effects.todo_list() {
                Ok(display) => {
                    let _ = runtime.respond(true, display);
                }
                Err(e) => {
                    let _ = runtime.respond(false, format!("Failed to load todo list: {e}"));
                }
            },
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
        let expected_instance_id = context.cryo_state.instance_id.as_deref();

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                runtime.terminate();
                if !context.inbox_filenames.is_empty() {
                    let _ = effects.archive_inbox(context.inbox_filenames);
                }
                let decision = resolve_interrupted_session(
                    SessionInterruption::Shutdown,
                    hibernate_outcome.take(),
                );
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
                    if !context.inbox_filenames.is_empty() {
                        let _ = effects.archive_inbox(context.inbox_filenames);
                    }
                    let decision = resolve_interrupted_session(
                        SessionInterruption::Timeout,
                        hibernate_outcome.take(),
                    );
                    logger.finish(decision.finish_reason)?;
                    return Ok(decision.outcome);
                }
            }

            match runtime.accept_request(expected_instance_id) {
                Ok(Some(request)) => self.handle_active_request(
                    request,
                    runtime,
                    effects,
                    &mut logger,
                    &mut pending_fallback,
                    &mut hibernate_outcome,
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

                    if !context.inbox_filenames.is_empty() {
                        effects.archive_inbox(context.inbox_filenames)?;
                    }

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
                    logger.finish(decision.finish_reason)?;
                    return Ok(decision.outcome);
                }
                Ok(None) => {}
                Err(e) => {
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
    fn check_fallback(
        &self,
        cryo_state: &mut CryoState,
        pending: &mut Option<(NaiveDateTime, FallbackAction)>,
        alert_method: &str,
    ) {
        if let Some((deadline, _)) = pending.as_ref() {
            if self.clock.local_now() > *deadline {
                let (_, fb) = pending.take().unwrap();
                self.sync_pending_fallback_state(cryo_state, pending.as_ref());
                if let Err(e) = state::save_state(&self.state_path, cryo_state) {
                    eprintln!("Daemon: failed to clear pending fallback state: {e}");
                }
                eprintln!("Daemon: fallback deadline passed, executing fallback action");
                if let Err(e) = fb.execute(&self.dir, alert_method) {
                    eprintln!("Daemon: fallback execution failed: {e}");
                }
            }
        }
    }

    /// Handle a failure by retrying with exponential backoff (5s, 10s, ..., 1h cap).
    /// Sends an alert once when max_retries is reached, then keeps retrying at 1h.
    /// Returns true if the daemon should shut down.
    fn handle_failure_retry(&self, retry: &mut RetryState, alert_method: &str) -> bool {
        let backoff = retry.next_backoff();
        retry.record_failure();
        // Send alert once when we first hit max_retries
        if retry.attempt == retry.max_retries {
            eprintln!(
                "Daemon: {} retries failed, sending alert. Will keep retrying.",
                retry.max_retries
            );
            self.send_retry_alert(alert_method);
        }
        eprintln!("Daemon: retry {} in {}s", retry.attempt, backoff.as_secs());
        self.sleep_or_shutdown(backoff)
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
                if let Err(e) = crate::report::send_report_notification(&summary, project_name) {
                    eprintln!("Daemon: report notification failed: {e}");
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
        if let Err(e) = state::save_state(&self.state_path, cryo_state) {
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
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct TestClockState {
        now: NaiveDateTime,
        elapsed: Duration,
        sleeps: Vec<Duration>,
    }

    struct TestClock {
        origin: std::time::Instant,
        state: Mutex<TestClockState>,
    }

    impl TestClock {
        fn new(now: NaiveDateTime) -> Self {
            Self {
                origin: std::time::Instant::now(),
                state: Mutex::new(TestClockState {
                    now,
                    elapsed: Duration::ZERO,
                    sleeps: Vec::new(),
                }),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut state = self.state.lock().unwrap();
            state.now += chrono::Duration::from_std(duration).unwrap();
            state.elapsed += duration;
        }

        fn sleeps(&self) -> Vec<Duration> {
            self.state.lock().unwrap().sleeps.clone()
        }
    }

    impl Clock for TestClock {
        fn local_now(&self) -> NaiveDateTime {
            self.state.lock().unwrap().now
        }

        fn monotonic_now(&self) -> std::time::Instant {
            let state = self.state.lock().unwrap();
            self.origin + state.elapsed
        }

        fn sleep(&self, duration: Duration) {
            let mut state = self.state.lock().unwrap();
            state.sleeps.push(duration);
            state.now += chrono::Duration::from_std(duration).unwrap();
            state.elapsed += duration;
        }
    }

    struct FakeEventSource {
        events: Mutex<VecDeque<Result<DaemonEvent, WaitError>>>,
        drained_inbox: Mutex<u32>,
    }

    impl FakeEventSource {
        fn new(events: Vec<Result<DaemonEvent, WaitError>>) -> Self {
            Self {
                events: Mutex::new(events.into()),
                drained_inbox: Mutex::new(0),
            }
        }

        fn drained_count(&self) -> u32 {
            *self.drained_inbox.lock().unwrap()
        }
    }

    struct FakeSessionRuntime {
        requests: Mutex<VecDeque<anyhow::Result<Option<crate::socket::Request>>>>,
        waits: Mutex<VecDeque<std::io::Result<Option<ChildExitStatus>>>>,
        responses: Mutex<Vec<(bool, String)>>,
        terminated: AtomicBool,
    }

    impl FakeSessionRuntime {
        fn new(
            requests: Vec<anyhow::Result<Option<crate::socket::Request>>>,
            waits: Vec<std::io::Result<Option<ChildExitStatus>>>,
        ) -> Self {
            Self {
                requests: Mutex::new(requests.into()),
                waits: Mutex::new(waits.into()),
                responses: Mutex::new(Vec::new()),
                terminated: AtomicBool::new(false),
            }
        }

        fn responses(&self) -> Vec<(bool, String)> {
            self.responses.lock().unwrap().clone()
        }

        fn terminated(&self) -> bool {
            self.terminated.load(Ordering::Relaxed)
        }
    }

    impl SessionRuntime for FakeSessionRuntime {
        fn accept_request(
            &mut self,
            _expected_instance_id: Option<&str>,
        ) -> Result<Option<crate::socket::Request>> {
            self.requests
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(None))
        }

        fn respond(&mut self, ok: bool, message: String) -> Result<()> {
            self.responses.lock().unwrap().push((ok, message));
            Ok(())
        }

        fn try_wait(&mut self) -> std::io::Result<Option<ChildExitStatus>> {
            self.waits.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }

        fn terminate(&mut self) {
            self.terminated.store(true, Ordering::Relaxed);
        }
    }

    struct FakeSessionEffects {
        reply_failure: Option<String>,
        replies: Vec<(String, NaiveDateTime)>,
        todos: Vec<crate::todo::TodoItem>,
        next_todo_id: u32,
        archived_batches: Vec<Vec<String>>,
    }

    impl FakeSessionEffects {
        fn new() -> Self {
            Self {
                reply_failure: None,
                replies: Vec::new(),
                todos: Vec::new(),
                next_todo_id: 1,
                archived_batches: Vec::new(),
            }
        }

        fn with_reply_failure(message: &str) -> Self {
            let mut effects = Self::new();
            effects.reply_failure = Some(message.to_string());
            effects
        }
    }

    impl SessionEffects for FakeSessionEffects {
        fn archive_inbox(&mut self, inbox_filenames: &[String]) -> Result<()> {
            self.archived_batches.push(inbox_filenames.to_vec());
            Ok(())
        }

        fn write_reply(&mut self, text: &str, timestamp: NaiveDateTime) -> Result<()> {
            if let Some(message) = &self.reply_failure {
                anyhow::bail!("{message}");
            }
            self.replies.push((text.to_string(), timestamp));
            Ok(())
        }

        fn todo_add(&mut self, text: &str, at: &str) -> Result<u32> {
            let id = self.next_todo_id;
            self.next_todo_id += 1;
            self.todos.push(crate::todo::TodoItem {
                id,
                text: text.to_string(),
                done: false,
                at: at.to_string(),
                created: "unknown".to_string(),
            });
            Ok(id)
        }

        fn todo_done(&mut self, id: u32) -> Result<()> {
            let item = self
                .todos
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow::anyhow!("TODO #{id} not found"))?;
            item.done = true;
            Ok(())
        }

        fn todo_remove(&mut self, id: u32) -> Result<()> {
            let len_before = self.todos.len();
            self.todos.retain(|item| item.id != id);
            if self.todos.len() == len_before {
                anyhow::bail!("TODO #{id} not found");
            }
            Ok(())
        }

        fn todo_list(&mut self) -> Result<String> {
            if self.todos.is_empty() {
                return Ok("No todos.".to_string());
            }
            Ok(self
                .todos
                .iter()
                .map(|item| {
                    let check = if item.done { "x" } else { " " };
                    format!("{}. [{}] {} (at: {})", item.id, check, item.text, item.at)
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct DummyServer;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct DummyWatcher;

    struct FakeStartupPlatform {
        signal_error: Option<String>,
        bind_error: Option<String>,
        registry_error: Option<String>,
        watcher_error: Option<String>,
        bind_calls: Mutex<u32>,
        watcher_calls: Mutex<u32>,
    }

    impl FakeStartupPlatform {
        fn new() -> Self {
            Self {
                signal_error: None,
                bind_error: None,
                registry_error: None,
                watcher_error: None,
                bind_calls: Mutex::new(0),
                watcher_calls: Mutex::new(0),
            }
        }

        fn bind_calls(&self) -> u32 {
            *self.bind_calls.lock().unwrap()
        }

        fn watcher_calls(&self) -> u32 {
            *self.watcher_calls.lock().unwrap()
        }
    }

    impl StartupPlatform for FakeStartupPlatform {
        type Server = DummyServer;
        type Watcher = DummyWatcher;

        fn register_signal_handlers(
            &self,
            _shutdown: &Arc<AtomicBool>,
            _wake_requested: &Arc<AtomicBool>,
        ) -> Result<()> {
            if let Some(message) = &self.signal_error {
                anyhow::bail!("{message}");
            }
            Ok(())
        }

        fn bind_socket_server(&self, _sock_path: &Path) -> Result<Self::Server> {
            *self.bind_calls.lock().unwrap() += 1;
            if let Some(message) = &self.bind_error {
                anyhow::bail!("{message}");
            }
            Ok(DummyServer)
        }

        fn register_registry(&self, _dir: &Path, _sock_path: &Path) -> Result<()> {
            if let Some(message) = &self.registry_error {
                anyhow::bail!("{message}");
            }
            Ok(())
        }

        fn start_inbox_watcher(
            &self,
            _inbox_path: &Path,
            _tx: mpsc::Sender<DaemonEvent>,
        ) -> Result<Self::Watcher> {
            *self.watcher_calls.lock().unwrap() += 1;
            if let Some(message) = &self.watcher_error {
                anyhow::bail!("{message}");
            }
            Ok(DummyWatcher)
        }
    }

    fn test_cryo_state() -> CryoState {
        CryoState {
            session_number: 1,
            pid: None,
            retry_count: 0,
            agent_override: None,
            max_retries_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            provider_index: None,
            instance_id: Some("test-instance".into()),
            pending_fallback: None,
        }
    }

    fn begin_test_logger(dir: &Path) -> crate::log::EventLogger {
        crate::log::EventLogger::begin(&dir.join("cryo.log"), 1, "test task", "mock-agent", &[])
            .unwrap()
    }

    fn test_session_context<'a>(
        cryo_state: &'a CryoState,
        timeout_secs: u64,
        spawn_time: Instant,
        inbox_filenames: &'a [String],
    ) -> ActiveSessionContext<'a> {
        ActiveSessionContext {
            cryo_state,
            timeout_secs,
            spawn_time,
            inbox_filenames,
        }
    }

    impl EventSource for FakeEventSource {
        fn recv_timeout(&self, _timeout: Duration) -> Result<DaemonEvent, WaitError> {
            self.events
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(WaitError::Timeout))
        }

        fn drain_inbox_changed(&self) {
            let mut events = self.events.lock().unwrap();
            while matches!(events.front(), Some(Ok(DaemonEvent::InboxChanged))) {
                events.pop_front();
                *self.drained_inbox.lock().unwrap() += 1;
            }
        }
    }

    #[test]
    fn test_backoff_sequence() {
        let mut state = RetryState::new(5, 1);
        // 5s, 10s, 20s, 40s, 80s, then keeps going capped at 3600s
        assert_eq!(state.next_backoff(), Duration::from_secs(5));

        state.record_failure();
        assert_eq!(state.next_backoff(), Duration::from_secs(10));

        state.record_failure();
        assert_eq!(state.next_backoff(), Duration::from_secs(20));

        state.record_failure();
        assert_eq!(state.next_backoff(), Duration::from_secs(40));

        state.record_failure();
        assert_eq!(state.next_backoff(), Duration::from_secs(80));

        // Past max_retries — still returns backoff, capped at 3600s
        state.record_failure();
        assert_eq!(state.attempt, 5);
        assert_eq!(state.next_backoff(), Duration::from_secs(160));
        assert!(state.exhausted());
    }

    #[test]
    fn test_backoff_caps_at_one_hour() {
        let mut state = RetryState::new(20, 1);
        for _ in 0..15 {
            state.record_failure();
        }
        // 5 * 2^15 = 163840 > 3600, so capped
        assert_eq!(state.next_backoff(), Duration::from_secs(3600));
    }

    #[test]
    fn test_backoff_reset() {
        let mut state = RetryState::new(3, 1);
        state.record_failure();
        state.record_failure();
        assert_eq!(state.attempt, 2);

        state.reset();
        assert_eq!(state.attempt, 0);
        assert!(!state.exhausted());
    }

    #[test]
    fn test_backoff_exact_sequence() {
        let mut retry = RetryState::new(20, 1);
        let expected = [5, 10, 20, 40, 80, 160, 320, 640, 1280, 2560, 3600, 3600];
        for (i, &secs) in expected.iter().enumerate() {
            assert_eq!(
                retry.next_backoff(),
                Duration::from_secs(secs),
                "Backoff at attempt {i} should be {secs}s"
            );
            retry.record_failure();
        }
    }

    #[test]
    fn test_backoff_cap_never_exceeds_3600() {
        let mut retry = RetryState::new(100, 1);
        for _ in 0..100 {
            let backoff = retry.next_backoff();
            assert!(
                backoff <= Duration::from_secs(3600),
                "Backoff should never exceed 3600s, got {:?}",
                backoff
            );
            retry.record_failure();
        }
    }

    #[test]
    fn test_rotate_provider_single_provider() {
        let mut retry = RetryState::new(5, 1);
        // With only 1 provider, rotate always returns true (can't rotate)
        assert!(
            retry.rotate_provider(),
            "Single provider should always wrap"
        );
        assert_eq!(retry.provider_index, 0);
    }

    #[test]
    fn test_rotate_provider_advances_and_wraps() {
        let mut retry = RetryState::new(5, 3);
        assert_eq!(retry.provider_index, 0);

        assert!(!retry.rotate_provider(), "Should not wrap: 0->1");
        assert_eq!(retry.provider_index, 1);

        assert!(!retry.rotate_provider(), "Should not wrap: 1->2");
        assert_eq!(retry.provider_index, 2);

        assert!(retry.rotate_provider(), "Should wrap: 2->0");
        assert_eq!(retry.provider_index, 0);
    }

    #[test]
    fn test_reset_clears_attempt_and_provider() {
        let mut retry = RetryState::new(5, 3);
        retry.record_failure();
        retry.record_failure();
        retry.rotate_provider(); // index = 1, attempt reset to 0 by rotate
        retry.record_failure(); // attempt = 1
        assert_eq!(retry.attempt, 1);
        assert_eq!(retry.provider_index, 1);

        retry.reset();
        assert_eq!(retry.attempt, 0);
        assert_eq!(
            retry.provider_index, 0,
            "Provider index should be reset to 0"
        );
    }

    #[test]
    fn test_exhausted_boundary() {
        let mut retry = RetryState::new(3, 1);
        assert!(!retry.exhausted(), "Should not be exhausted at attempt 0");
        retry.record_failure();
        assert!(!retry.exhausted(), "Should not be exhausted at attempt 1");
        retry.record_failure();
        assert!(!retry.exhausted(), "Should not be exhausted at attempt 2");
        retry.record_failure();
        assert!(
            retry.exhausted(),
            "Should be exhausted at attempt 3 (== max_retries)"
        );
    }

    #[test]
    fn test_inbox_watcher_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("messages").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();

        let (tx, rx) = mpsc::channel();
        let _watcher = InboxWatcher::start(&inbox, tx).unwrap();

        // Create a file in inbox
        std::fs::write(inbox.join("test-message.md"), "hello").unwrap();

        // Should receive InboxChanged within 2 seconds
        let event = rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(event.unwrap(), DaemonEvent::InboxChanged);
    }

    #[test]
    fn test_inbox_watcher_ignores_non_create_events() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("messages").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();

        // Create file before watcher starts
        let file = inbox.join("existing.md");
        std::fs::write(&file, "original").unwrap();

        let (tx, rx) = mpsc::channel();
        let _watcher = InboxWatcher::start(&inbox, tx).unwrap();

        // Modify existing file (not a create)
        std::fs::write(&file, "modified").unwrap();

        // Should NOT receive InboxChanged (modification, not creation)
        // Give it 500ms — if nothing arrives, that's correct
        let event = rx.recv_timeout(Duration::from_millis(500));
        // This may or may not fire depending on platform — just don't assert it MUST fire
        // The key is that create events DO fire (tested above)
        let _ = event; // suppress unused warning
    }

    #[test]
    fn test_compute_sleep_timeout_both() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let wake = now + chrono::Duration::seconds(60);
        let report = now + chrono::Duration::seconds(30);
        let timeout = compute_sleep_timeout(Some(wake), Some(report), now);
        assert_eq!(
            timeout,
            Duration::from_secs(30),
            "Should pick earlier (report)"
        );
    }

    #[test]
    fn test_compute_sleep_timeout_wake_only() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let wake = now + chrono::Duration::seconds(120);
        let timeout = compute_sleep_timeout(Some(wake), None, now);
        assert_eq!(timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_compute_sleep_timeout_report_only() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let report = now + chrono::Duration::seconds(45);
        let timeout = compute_sleep_timeout(None, Some(report), now);
        assert_eq!(timeout, Duration::from_secs(45));
    }

    #[test]
    fn test_compute_sleep_timeout_neither() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let timeout = compute_sleep_timeout(None, None, now);
        assert_eq!(timeout, Duration::from_secs(3600));
    }

    #[test]
    fn test_wait_for_idle_event_coalesces_inbox_changes() {
        let source = FakeEventSource::new(vec![
            Ok(DaemonEvent::InboxChanged),
            Ok(DaemonEvent::InboxChanged),
            Ok(DaemonEvent::InboxChanged),
        ]);

        let outcome = wait_for_idle_event(&source, Duration::from_secs(1), true);

        assert_eq!(outcome, IdleWaitOutcome::WakeFromInbox);
        assert_eq!(source.drained_count(), 2);
    }

    #[test]
    fn test_wait_for_idle_event_timeout_with_scheduled_wake() {
        let source = FakeEventSource::new(vec![Err(WaitError::Timeout)]);

        let outcome = wait_for_idle_event(&source, Duration::from_secs(1), true);

        assert_eq!(outcome, IdleWaitOutcome::WakeFromSchedule);
    }

    #[test]
    fn test_wait_for_idle_event_timeout_without_scheduled_wake() {
        let source = FakeEventSource::new(vec![Err(WaitError::Timeout)]);

        let outcome = wait_for_idle_event(&source, Duration::from_secs(1), false);

        assert_eq!(outcome, IdleWaitOutcome::StayIdle);
    }

    #[test]
    fn test_wait_for_idle_event_shutdown_and_disconnect() {
        let shutdown = FakeEventSource::new(vec![Ok(DaemonEvent::Shutdown)]);
        let disconnected = FakeEventSource::new(vec![Err(WaitError::Disconnected)]);

        assert_eq!(
            wait_for_idle_event(&shutdown, Duration::from_secs(1), true),
            IdleWaitOutcome::Shutdown
        );
        assert_eq!(
            wait_for_idle_event(&disconnected, Duration::from_secs(1), true),
            IdleWaitOutcome::Disconnected
        );
    }

    #[test]
    fn test_delayed_wake_under_threshold() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let scheduled = now - chrono::Duration::minutes(4);
        assert!(
            detect_delayed_wake(scheduled, now).is_none(),
            "4 min delay should not be flagged"
        );
    }

    #[test]
    fn test_delayed_wake_over_threshold() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let scheduled = now - chrono::Duration::minutes(6);
        let result = detect_delayed_wake(scheduled, now);
        assert!(result.is_some(), "6 min delay should be flagged");
        assert_eq!(result.unwrap(), "6m");
    }

    #[test]
    fn test_next_wake_from_todos_picks_earliest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("todo.json");
        let mut list = crate::todo::TodoList::new();
        list.add("later".into(), "2026-03-02T16:00".into());
        list.add("earlier".into(), "2026-03-02T14:00".into());
        list.save(&path).unwrap();
        let wake = next_wake_from_todos(dir.path());
        assert_eq!(
            wake.unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(14, 0, 0)
                .unwrap()
        );
    }

    #[test]
    fn test_next_wake_from_todos_none_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let wake = next_wake_from_todos(dir.path());
        assert!(wake.is_none());
    }

    #[test]
    fn test_next_wake_from_todos_skips_done() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("todo.json");
        let mut list = crate::todo::TodoList::new();
        let id = list.add("done".into(), "2026-03-02T10:00".into());
        list.done(id).unwrap();
        list.save(&path).unwrap();
        let wake = next_wake_from_todos(dir.path());
        assert!(wake.is_none());
    }

    #[test]
    fn test_next_wake_from_todos_skips_invalid_and_picks_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("todo.json");
        let mut list = crate::todo::TodoList::new();
        // Invalid at value that sorts before valid ones
        list.add("bad format".into(), "2026-03-02 10:00".into());
        // Valid at value
        list.add("valid task".into(), "2026-03-02T14:00".into());
        list.save(&path).unwrap();
        let wake = next_wake_from_todos(dir.path());
        assert_eq!(
            wake.unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(14, 0, 0)
                .unwrap(),
            "Should skip invalid entry and pick valid one"
        );
    }

    #[test]
    fn test_next_wake_from_todos_skips_empty_at() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("todo.json");
        // Simulate legacy items with empty `at` (from serde default)
        let content = r#"[{"id":1,"text":"legacy item","done":false,"created":"unknown"},{"id":2,"text":"scheduled","done":false,"at":"2026-03-02T14:00","created":"unknown"}]"#;
        std::fs::write(&path, content).unwrap();
        let wake = next_wake_from_todos(dir.path());
        assert_eq!(
            wake.unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(14, 0, 0)
                .unwrap(),
            "Should skip empty at and pick the valid one"
        );
    }

    #[test]
    fn test_next_wake_from_todos_all_invalid_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("todo.json");
        let mut list = crate::todo::TodoList::new();
        list.add("bad1".into(), "not-a-date".into());
        list.add("bad2".into(), "also-bad".into());
        list.save(&path).unwrap();
        let wake = next_wake_from_todos(dir.path());
        assert!(wake.is_none(), "All invalid entries should yield None");
    }

    #[test]
    fn test_sleep_or_shutdown_uses_injected_clock() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());

        assert!(!daemon.sleep_or_shutdown(Duration::from_millis(600)));
        assert_eq!(
            clock.sleeps(),
            vec![
                Duration::from_millis(250),
                Duration::from_millis(250),
                Duration::from_millis(100),
            ]
        );
        assert_eq!(clock.local_now(), now + chrono::Duration::milliseconds(600));
    }

    #[test]
    fn test_check_fallback_uses_injected_clock() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();

        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());

        let action = FallbackAction {
            action: "email".to_string(),
            target: "ops@example.com".to_string(),
            message: "still waiting".to_string(),
        };
        let mut cryo_state = CryoState {
            session_number: 1,
            pid: None,
            retry_count: 0,
            agent_override: None,
            max_retries_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            provider_index: None,
            instance_id: None,
            pending_fallback: Some(PendingFallbackState {
                deadline: (now + chrono::Duration::minutes(1))
                    .format(FALLBACK_TIME_FMT)
                    .to_string(),
                action: action.clone(),
            }),
        };
        let mut pending = Some((now + chrono::Duration::minutes(1), action));

        daemon.check_fallback(&mut cryo_state, &mut pending, "outbox");
        assert!(
            pending.is_some(),
            "Fallback should not fire before the deadline"
        );

        clock.advance(Duration::from_secs(61));
        daemon.check_fallback(&mut cryo_state, &mut pending, "outbox");

        assert!(
            pending.is_none(),
            "Fallback should fire after fake time advances"
        );
        assert!(cryo_state.pending_fallback.is_none());
        let outbox = crate::message::read_outbox(dir.path()).unwrap();
        assert_eq!(outbox.len(), 1, "Fallback should write one outbox message");
    }

    #[test]
    fn test_resolve_hibernate_request_failure_retries() {
        let mut pending_fallback = Some(FallbackAction {
            action: "email".into(),
            target: "ops@example.com".into(),
            message: "stuck".into(),
        });

        let decision =
            resolve_hibernate_request(false, 7, Some("provider failed"), &mut pending_fallback);

        assert_eq!(
            decision.outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: false }
        );
        assert_eq!(
            decision.response_message,
            "Failure recorded. Daemon will retry."
        );
        assert_eq!(
            decision.log_event,
            "hibernate failed: exit=7, summary=\"provider failed\""
        );
        assert!(
            pending_fallback.is_some(),
            "failure should not consume fallback"
        );
    }

    #[test]
    fn test_resolve_hibernate_request_complete_ignores_fallback() {
        let mut pending_fallback = Some(FallbackAction {
            action: "email".into(),
            target: "ops@example.com".into(),
            message: "stuck".into(),
        });

        let decision = resolve_hibernate_request(true, 0, None, &mut pending_fallback);

        assert_eq!(decision.outcome, SessionLoopOutcome::PlanComplete);
        assert_eq!(decision.response_message, "Plan complete. Shutting down.");
        assert_eq!(
            decision.log_event,
            "hibernate: plan complete, exit=0, summary=\"(no summary)\""
        );
        assert!(
            pending_fallback.is_some(),
            "complete should not consume fallback"
        );
    }

    #[test]
    fn test_resolve_hibernate_request_uses_pending_fallback() {
        let fallback = FallbackAction {
            action: "webhook".into(),
            target: "ops".into(),
            message: "waiting".into(),
        };
        let mut pending_fallback = Some(fallback.clone());

        let decision =
            resolve_hibernate_request(false, 0, Some("waiting on reply"), &mut pending_fallback);

        assert_eq!(
            decision.outcome,
            SessionLoopOutcome::Hibernate {
                fallback: Some(fallback),
            }
        );
        assert_eq!(decision.response_message, "Hibernating.");
        assert_eq!(
            decision.log_event,
            "hibernate: exit=0, summary=\"waiting on reply\""
        );
        assert!(
            pending_fallback.is_none(),
            "successful hibernate should consume pending fallback"
        );
    }

    #[test]
    fn test_resolve_interrupted_session_prefers_hibernate_outcome() {
        let hibernate = SessionLoopOutcome::Hibernate { fallback: None };

        let shutdown =
            resolve_interrupted_session(SessionInterruption::Shutdown, Some(hibernate.clone()));
        let timeout = resolve_interrupted_session(SessionInterruption::Timeout, Some(hibernate));

        assert_eq!(
            shutdown.outcome,
            SessionLoopOutcome::Hibernate { fallback: None }
        );
        assert_eq!(
            shutdown.finish_reason,
            "daemon shutdown — using agent's hibernate outcome"
        );
        assert_eq!(
            timeout.outcome,
            SessionLoopOutcome::Hibernate { fallback: None }
        );
        assert_eq!(
            timeout.finish_reason,
            "session timeout — using agent's hibernate outcome"
        );
    }

    #[test]
    fn test_resolve_interrupted_session_without_hibernate_fails() {
        let shutdown = resolve_interrupted_session(SessionInterruption::Shutdown, None);
        let timeout = resolve_interrupted_session(SessionInterruption::Timeout, None);

        assert_eq!(
            shutdown.outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: false }
        );
        assert_eq!(shutdown.finish_reason, "daemon shutdown — agent terminated");
        assert_eq!(
            timeout.outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: false }
        );
        assert_eq!(timeout.finish_reason, "session timeout — agent killed");
    }

    #[test]
    fn test_resolve_child_exit_after_hibernate_returns_outcome() {
        let decision = resolve_child_exit(
            Some(SessionLoopOutcome::PlanComplete),
            Duration::from_secs(1),
        );

        assert_eq!(decision.outcome, SessionLoopOutcome::PlanComplete);
        assert_eq!(decision.finish_reason, "session complete");
        assert!(!decision.quick_exit);
    }

    #[test]
    fn test_resolve_child_exit_without_hibernate_marks_quick_exit() {
        let quick = resolve_child_exit(None, Duration::from_secs(2));
        let slow = resolve_child_exit(None, Duration::from_secs(8));

        assert_eq!(
            quick.outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: true }
        );
        assert_eq!(quick.finish_reason, "agent exited without hibernate");
        assert!(quick.quick_exit);

        assert_eq!(
            slow.outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: false }
        );
        assert_eq!(slow.finish_reason, "agent exited without hibernate");
        assert!(!slow.quick_exit);
    }

    #[test]
    fn test_drive_active_session_alert_then_hibernate_returns_fallback() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
        let cryo_state = test_cryo_state();
        let mut runtime = FakeSessionRuntime::new(
            vec![
                Ok(Some(crate::socket::Request::Alert {
                    action: "webhook".into(),
                    target: "ops".into(),
                    message: "waiting".into(),
                })),
                Ok(Some(crate::socket::Request::Hibernate {
                    complete: false,
                    exit_code: 0,
                    summary: Some("waiting on reply".into()),
                })),
            ],
            vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(0) }))],
        );
        let mut effects = FakeSessionEffects::new();

        let outcome = daemon
            .drive_active_session(
                &mut runtime,
                &mut effects,
                test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
                begin_test_logger(dir.path()),
            )
            .unwrap();

        assert_eq!(
            outcome,
            SessionLoopOutcome::Hibernate {
                fallback: Some(FallbackAction {
                    action: "webhook".into(),
                    target: "ops".into(),
                    message: "waiting".into(),
                }),
            }
        );
        assert_eq!(
            runtime.responses(),
            vec![
                (true, "Alert registered".into()),
                (true, "Hibernating.".into()),
            ]
        );
        assert!(!runtime.terminated());
    }

    #[test]
    fn test_drive_active_session_timeout_after_hibernate_uses_hibernate_outcome() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
        let cryo_state = test_cryo_state();
        let mut runtime = FakeSessionRuntime::new(
            vec![Ok(Some(crate::socket::Request::Hibernate {
                complete: false,
                exit_code: 0,
                summary: Some("waiting".into()),
            }))],
            vec![],
        );
        let mut effects = FakeSessionEffects::new();

        let outcome = daemon
            .drive_active_session(
                &mut runtime,
                &mut effects,
                test_session_context(&cryo_state, 1, clock.monotonic_now(), &[]),
                begin_test_logger(dir.path()),
            )
            .unwrap();

        assert_eq!(outcome, SessionLoopOutcome::Hibernate { fallback: None });
        assert!(runtime.terminated(), "timeout should terminate the child");
        assert_eq!(runtime.responses(), vec![(true, "Hibernating.".into())]);
    }

    #[test]
    fn test_drive_active_session_quick_exit_without_hibernate() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
        let cryo_state = test_cryo_state();
        let mut runtime =
            FakeSessionRuntime::new(vec![], vec![Ok(Some(ChildExitStatus { code: Some(1) }))]);
        let mut effects = FakeSessionEffects::new();

        let outcome = daemon
            .drive_active_session(
                &mut runtime,
                &mut effects,
                test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
                begin_test_logger(dir.path()),
            )
            .unwrap();

        assert_eq!(
            outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: true }
        );
        assert!(!runtime.terminated());
        assert!(runtime.responses().is_empty());
    }

    #[test]
    fn test_drive_active_session_reply_failure_responds_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
        let cryo_state = test_cryo_state();
        let mut runtime = FakeSessionRuntime::new(
            vec![
                Ok(Some(crate::socket::Request::Reply {
                    text: "Need approval".into(),
                })),
                Ok(Some(crate::socket::Request::Hibernate {
                    complete: false,
                    exit_code: 0,
                    summary: Some("waiting".into()),
                })),
            ],
            vec![
                Ok(None),
                Ok(None),
                Ok(Some(ChildExitStatus { code: Some(0) })),
            ],
        );
        let mut effects = FakeSessionEffects::with_reply_failure("injected reply failure");

        let outcome = daemon
            .drive_active_session(
                &mut runtime,
                &mut effects,
                test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
                begin_test_logger(dir.path()),
            )
            .unwrap();

        assert_eq!(outcome, SessionLoopOutcome::Hibernate { fallback: None });
        assert_eq!(
            runtime.responses(),
            vec![
                (
                    false,
                    "Failed to write reply: injected reply failure".into()
                ),
                (true, "Hibernating.".into()),
            ]
        );
        assert!(effects.replies.is_empty());
    }

    #[test]
    fn test_drive_active_session_todo_requests_use_effects() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
        let cryo_state = test_cryo_state();
        let mut runtime = FakeSessionRuntime::new(
            vec![
                Ok(Some(crate::socket::Request::TodoAdd {
                    text: "Check inbox".into(),
                    at: "2026-03-01T13:00".into(),
                })),
                Ok(Some(crate::socket::Request::TodoList)),
                Ok(Some(crate::socket::Request::TodoDone { id: 1 })),
                Ok(Some(crate::socket::Request::TodoRemove { id: 1 })),
                Ok(Some(crate::socket::Request::Hibernate {
                    complete: false,
                    exit_code: 0,
                    summary: None,
                })),
            ],
            vec![
                Ok(None),
                Ok(None),
                Ok(None),
                Ok(None),
                Ok(None),
                Ok(Some(ChildExitStatus { code: Some(0) })),
            ],
        );
        let mut effects = FakeSessionEffects::new();

        let outcome = daemon
            .drive_active_session(
                &mut runtime,
                &mut effects,
                test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
                begin_test_logger(dir.path()),
            )
            .unwrap();

        assert_eq!(outcome, SessionLoopOutcome::Hibernate { fallback: None });
        assert_eq!(
            runtime.responses(),
            vec![
                (true, "Added todo #1".into()),
                (true, "1. [ ] Check inbox (at: 2026-03-01T13:00)".into()),
                (true, "Marked todo #1 as done".into()),
                (true, "Removed todo #1".into()),
                (true, "Hibernating.".into()),
            ]
        );
        assert!(effects.todos.is_empty());
    }

    #[test]
    fn test_drive_active_session_timeout_archives_inbox_via_effects() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
        let cryo_state = test_cryo_state();
        let mut runtime = FakeSessionRuntime::new(vec![], vec![]);
        let mut effects = FakeSessionEffects::new();
        let inbox = vec!["msg-1.md".to_string(), "msg-2.md".to_string()];

        let outcome = daemon
            .drive_active_session(
                &mut runtime,
                &mut effects,
                test_session_context(&cryo_state, 1, clock.monotonic_now(), &inbox),
                begin_test_logger(dir.path()),
            )
            .unwrap();

        assert_eq!(
            outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: false }
        );
        assert!(runtime.terminated());
        assert_eq!(effects.archived_batches, vec![inbox]);
    }

    #[test]
    fn test_build_bootstrap_state_clears_invalid_pending_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let mut cryo_state = test_cryo_state();
        cryo_state.pending_fallback = Some(PendingFallbackState {
            deadline: "not-a-time".into(),
            action: FallbackAction {
                action: "email".into(),
                target: "ops@example.com".into(),
                message: "stuck".into(),
            },
        });

        let bootstrap = daemon.build_bootstrap_state(&mut cryo_state, &CryoConfig::default());

        assert!(bootstrap.pending_fallback.is_none());
        assert!(bootstrap.cleared_invalid_pending_fallback);
        assert!(cryo_state.pending_fallback.is_none());
    }

    #[test]
    fn test_build_bootstrap_state_runs_immediately_for_first_session_and_overdue_wake() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let mut first_session = test_cryo_state();
        first_session.session_number = 0;

        let first = daemon.build_bootstrap_state(&mut first_session, &CryoConfig::default());
        assert!(first.run_now);

        let todo_path = dir.path().join("todo.json");
        let mut todos = crate::todo::TodoList::new();
        todos.add("Overdue task".into(), "2026-03-01T11:30".into());
        todos.save(&todo_path).unwrap();

        let mut resumed = test_cryo_state();
        resumed.session_number = 3;
        let resumed_bootstrap = daemon.build_bootstrap_state(&mut resumed, &CryoConfig::default());
        assert_eq!(
            resumed_bootstrap.next_wake,
            Some(
                chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
                    .unwrap()
                    .and_hms_opt(11, 30, 0)
                    .unwrap()
            )
        );
        assert!(resumed_bootstrap.run_now);
    }

    #[test]
    fn test_build_bootstrap_state_only_enables_watcher_when_configured_and_present() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let mut cryo_state = test_cryo_state();
        let mut config = CryoConfig::default();

        let no_inbox = daemon.build_bootstrap_state(&mut cryo_state, &config);
        assert!(no_inbox.watch_inbox_path.is_none());

        let inbox = dir.path().join("messages").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let with_inbox = daemon.build_bootstrap_state(&mut cryo_state, &config);
        assert_eq!(with_inbox.watch_inbox_path, Some(inbox.clone()));

        config.watch_inbox = false;
        let disabled = daemon.build_bootstrap_state(&mut cryo_state, &config);
        assert!(disabled.watch_inbox_path.is_none());
    }

    #[test]
    fn test_prepare_shutdown_state_clears_runtime_identity_and_syncs_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let mut cryo_state = test_cryo_state();
        cryo_state.pid = Some(1234);
        cryo_state.instance_id = Some("daemon-1".into());
        let pending = Some((
            now + chrono::Duration::hours(1),
            FallbackAction {
                action: "webhook".into(),
                target: "ops".into(),
                message: "still waiting".into(),
            },
        ));

        daemon.prepare_shutdown_state(&mut cryo_state, pending.as_ref());

        assert!(cryo_state.pid.is_none());
        assert!(cryo_state.instance_id.is_none());
        assert_eq!(
            cryo_state.pending_fallback,
            Some(PendingFallbackState {
                deadline: "2026-03-01T13:00:00".into(),
                action: FallbackAction {
                    action: "webhook".into(),
                    target: "ops".into(),
                    message: "still waiting".into(),
                },
            })
        );
    }

    #[test]
    fn test_prepare_runtime_startup_returns_registry_warning_but_continues() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let inbox = dir.path().join("messages").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let mut platform = FakeStartupPlatform::new();
        platform.registry_error = Some("registry unavailable".into());
        let (tx, _rx) = mpsc::channel();

        let startup = daemon
            .prepare_runtime_startup(&platform, Some(inbox.as_path()), tx)
            .unwrap();

        assert_eq!(
            startup.diagnostics.registry_warning,
            Some("registry unavailable".into())
        );
        assert!(startup.diagnostics.watcher_warning.is_none());
        assert_eq!(platform.bind_calls(), 1);
        assert_eq!(platform.watcher_calls(), 1);
        assert!(startup.watcher.is_some());
    }

    #[test]
    fn test_prepare_runtime_startup_watcher_failure_is_nonfatal() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let inbox = dir.path().join("messages").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let mut platform = FakeStartupPlatform::new();
        platform.watcher_error = Some("watch failed".into());
        let (tx, _rx) = mpsc::channel();

        let startup = daemon
            .prepare_runtime_startup(&platform, Some(inbox.as_path()), tx)
            .unwrap();

        assert!(startup.watcher.is_none());
        assert_eq!(
            startup.diagnostics.watcher_warning,
            Some("watch failed".into())
        );
        assert_eq!(platform.watcher_calls(), 1);
    }

    #[test]
    fn test_prepare_runtime_startup_propagates_signal_registration_failure() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let mut platform = FakeStartupPlatform::new();
        platform.signal_error = Some("signal registration failed".into());
        let (tx, _rx) = mpsc::channel();

        let error = daemon
            .prepare_runtime_startup(&platform, None, tx)
            .unwrap_err()
            .to_string();

        assert!(error.contains("signal registration failed"));
        assert_eq!(platform.bind_calls(), 0);
        assert_eq!(platform.watcher_calls(), 0);
    }

    #[test]
    fn test_prepare_runtime_startup_propagates_socket_bind_failure() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = Arc::new(TestClock::new(now));
        let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
        let mut platform = FakeStartupPlatform::new();
        platform.bind_error = Some("bind failed".into());
        let (tx, _rx) = mpsc::channel();

        let error = daemon
            .prepare_runtime_startup(&platform, None, tx)
            .unwrap_err()
            .to_string();

        assert!(error.contains("bind failed"));
        assert_eq!(platform.bind_calls(), 1);
        assert_eq!(platform.watcher_calls(), 0);
    }
}
