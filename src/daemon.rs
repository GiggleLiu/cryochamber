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
    Hibernate,
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
mod inbox;
pub(super) mod dialog;
mod request;
mod schedule;
mod session;

use effects::{ReplyAuthor, SessionEffects};
use inbox::SessionInboxState;
pub use schedule::RetryState;
use schedule::{
    compute_sleep_timeout, decide_next_step, delayed_wake_notice, next_wake_from_todos,
    DaemonBootstrapState, NextStep, SessionRunResult,
};
#[cfg(test)]
use schedule::{
    detect_delayed_wake, should_rotate_provider, ProviderRotationReason, WAKE_TIME_FMT,
};

#[cfg(test)]
use request::TodoRequest;
use request::{
    handle_receive_request, handle_todo_request, resolve_hibernate_request, DaemonRequest,
    FileMessageEffects, FileTodoEffects, ReceiveRequestOutcome, TodoRequestOutcome,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Break,
    Continue,
    Idle,
}

struct ActiveSessionContext<'a> {
    cryo_state: &'a CryoState,
    timeout_secs: u64,
    spawn_time: Instant,
}

struct ActiveRequestState<'a> {
    logger: &'a mut crate::log::EventLogger,
    hibernate_outcome: &'a mut Option<SessionLoopOutcome>,
    inbox_state: &'a mut SessionInboxState,
}

struct EventLoopMutations<'a> {
    cryo_state: &'a mut CryoState,
    retry: &'a mut RetryState,
    next_wake: &'a mut Option<NaiveDateTime>,
    run_now: &'a mut bool,
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

fn daemon_missing_outbound_text() -> &'static str {
    "The agent completed this session without sending an outbox message. \
     The daemon is sending this status so every agent run has a visible message."
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

        DaemonBootstrapState {
            next_report_time,
            next_wake,
            run_now,
            watch_inbox_path,
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
            DaemonRequest::Hello { protocol_version } => {
                let _ = responder.respond(&ipc_protocol_response(protocol_version));
            }
            DaemonRequest::Dialog { .. } => {
                let _ = responder.respond(&crate::socket::Response {
                    ok: false,
                    message: "dialog not yet implemented".into(),
                });
            }
            DaemonRequest::Todo(todo_request) => {
                let mut effects = FileTodoEffects::new(&self.dir);
                let response = handle_todo_request(todo_request, &mut effects).into_response();
                let _ = responder.respond(&response);
            }
            DaemonRequest::Receive => {
                let mut effects = FileMessageEffects::new(&self.dir);
                let outcome = handle_receive_request(&mut effects);
                let _ = responder.respond(&outcome.into_response());
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
        config: &CryoConfig,
        state: EventLoopMutations<'_>,
    ) -> Result<LoopControl> {
        match step {
            NextStep::PlanComplete => {
                state.retry.reset();
                eprintln!("Daemon: plan complete. Shutting down.");
                Ok(LoopControl::Break)
            }
            NextStep::Hibernate {
                next_wake: refreshed_next_wake,
            } => {
                state.retry.reset();
                *state.next_wake = refreshed_next_wake;
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
                self.save_state_or_log(state.cryo_state, "persist provider rotation");

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
            bootstrap.watch_inbox_path.as_deref(),
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
        let mut retry = RetryState::new(provider_count);
        let mut next_wake = bootstrap.next_wake;
        let mut run_now = bootstrap.run_now;
        let mut inbox_wake = false;

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
                cryo_state.session_number += 1;
                cryo_state.session_active = true;
                if !config.providers.is_empty() {
                    cryo_state.provider_index = Some(retry.provider_index);
                }
                self.save_state_or_log(cryo_state, "persist session-active state");

                // Build provider env for this session
                let active_provider = config.providers.get(retry.provider_index);
                let provider_env: std::collections::HashMap<String, String> =
                    active_provider.map(|p| p.env.clone()).unwrap_or_default();
                let provider_name = active_provider.map(|p| p.name.as_str());

                // Claim past-due TODOs before spawning the agent so the
                // prompt shows exactly what triggered the wake while the
                // scheduler ignores those items until this session ends.
                self.claim_past_due_todos();

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
                        cryo_state.session_active = false;
                        if outcome.is_crash() {
                            self.reschedule_claimed_after_crash();
                        } else {
                            self.complete_claimed_todos();
                        }
                        // Persist session number only after successful completion
                        self.save_state_or_log(cryo_state, "persist completed session state");
                        Ok(outcome)
                    }
                    Err(e) => {
                        cryo_state.session_number -= 1;
                        cryo_state.session_active = false;
                        cryo_state.previous_session_crashed = true;
                        self.reschedule_claimed_after_crash();
                        self.save_state_or_log(cryo_state, "persist failed session state");
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
                runtime.respond(true, "pong".into())?;
            }
            DaemonRequest::Hello { protocol_version } => {
                let response = ipc_protocol_response(protocol_version);
                runtime.respond(response.ok, response.message)?;
            }
            DaemonRequest::Dialog { .. } => {
                runtime.respond(false, "dialog not yet implemented".into())?;
            }
            DaemonRequest::Send { text } => {
                let has_claimed_batch = state.inbox_state.has_claimed_batch();
                match effects.write_reply(ReplyAuthor::Agent, &text, self.clock.local_now()) {
                    Ok(()) => {
                        if has_claimed_batch {
                            state.inbox_state.complete_agent_send();
                        } else {
                            state.inbox_state.record_status_send();
                        }
                        state.logger.log_event(&format!("send: \"{text}\""))?;
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
                let decision = resolve_hibernate_request(
                    complete,
                    exit_code,
                    summary.as_deref(),
                    effects.has_pending_todo_with_valid_wake(),
                );
                state.logger.log_event(&decision.log_event)?;
                if let Some(outcome) = decision.outcome {
                    *state.hibernate_outcome = Some(outcome);
                }
                runtime.respond(decision.response_ok, decision.response_message.into())?;
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
            DaemonRequest::Todo(todo_request) => {
                let TodoRequestOutcome {
                    ok,
                    message,
                    log_event,
                } = handle_todo_request(todo_request, effects);
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
    ) {
        let mut daemon_wrote_reply = false;
        let message_count = inbox_state.claimed_message_count();
        if message_count > 0 {
            let text = daemon_unanswered_reply_text(message_count);
            match effects.write_reply(ReplyAuthor::Daemon, &text, self.clock.local_now()) {
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

        if message_count == 0 && !inbox_state.has_agent_outbound_message() && !daemon_wrote_reply {
            match effects.write_reply(
                ReplyAuthor::Daemon,
                daemon_missing_outbound_text(),
                self.clock.local_now(),
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
        let mut inbox_state = SessionInboxState::new();
        let expected_instance_id = context.cryo_state.instance_id.as_deref();

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                runtime.terminate();
                let decision = resolve_interrupted_session(
                    SessionInterruption::Shutdown,
                    hibernate_outcome.take(),
                );
                self.finalize_human_replies(effects, &mut logger, &mut inbox_state);
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
                    self.finalize_human_replies(effects, &mut logger, &mut inbox_state);
                    logger.finish(decision.finish_reason)?;
                    return Ok(decision.outcome);
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
                        },
                    ) {
                        self.finalize_human_replies(effects, &mut logger, &mut inbox_state);
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
                    self.finalize_human_replies(effects, &mut logger, &mut inbox_state);
                    logger.finish(decision.finish_reason)?;
                    return Ok(decision.outcome);
                }
                Ok(None) => {}
                Err(e) => {
                    self.finalize_human_replies(effects, &mut logger, &mut inbox_state);
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

    fn get_task(&self) -> Option<String> {
        crate::log::parse_latest_session_task(&self.log_path)
            .ok()
            .flatten()
    }

    /// Claim past-due pending TODOs so the prompt can show them as in-flight
    /// while the scheduler ignores them. Load/save failures are swallowed and
    /// reported to stderr — TODO bookkeeping must never abort the session loop.
    fn claim_past_due_todos(&self) {
        let now = self.clock.local_now();
        match crate::todo::TodoFile::new(self.dir.join("todo.json")).claim_due(&now) {
            Ok(items) if !items.is_empty() => {
                eprintln!("Daemon: claimed {} due TODO(s)", items.len());
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Daemon: failed to claim TODO list: {e}");
            }
        }
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
#[path = "unit_tests/daemon/dialog.rs"]
mod dialog_tests;

#[cfg(test)]
#[path = "unit_tests/daemon/request.rs"]
mod request_tests;

#[cfg(test)]
#[path = "unit_tests/daemon_properties.rs"]
mod property_tests;
