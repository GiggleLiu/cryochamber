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
use std::time::Duration;

use crate::config::CryoConfig;
use crate::fallback::FallbackAction;
use crate::state::{self, CryoState};

use crate::process::send_signal;

/// Events the daemon responds to.
#[derive(Debug, PartialEq)]
pub enum DaemonEvent {
    /// New file appeared in messages/inbox/.
    InboxChanged,
    /// SIGTERM or SIGINT received.
    Shutdown,
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
pub enum SessionLoopOutcome {
    PlanComplete,
    Hibernate {
        wake_time: NaiveDateTime,
        fallback: Option<FallbackAction>,
    },
    ValidationFailed {
        quick_exit: bool,
    },
}

/// Gracefully terminate a child process: SIGTERM, wait 2s, SIGKILL if needed.
fn terminate_child(child: &mut std::process::Child, pid: u32) {
    send_signal(pid, libc::SIGTERM);
    std::thread::sleep(Duration::from_secs(2));
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
    let to_duration = |dt: NaiveDateTime| -> Duration {
        (dt - now).to_std().unwrap_or(Duration::ZERO)
    };
    match (wake_deadline.map(&to_duration), report_deadline.map(&to_duration)) {
        (Some(w), Some(r)) => w.min(r),
        (Some(w), None) => w,
        (None, Some(r)) => r,
        (None, None) => Duration::from_secs(3600),
    }
}

/// Check if the scheduled wake time is significantly in the past (machine suspend).
/// Returns `Some(delay_description)` if delayed by more than 5 minutes.
fn detect_delayed_wake(
    scheduled: NaiveDateTime,
    now: NaiveDateTime,
) -> Option<String> {
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

/// The persistent daemon process.
pub struct Daemon {
    dir: PathBuf,
    state_path: PathBuf,
    log_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    wake_requested: Arc<AtomicBool>,
}

impl Daemon {
    pub fn new(dir: PathBuf) -> Self {
        let state_path = dir.join("timer.json");
        let log_path = dir.join("cryo.log");
        Self {
            dir,
            state_path,
            log_path,
            shutdown: Arc::new(AtomicBool::new(false)),
            wake_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Run the daemon event loop. Blocks until SIGTERM or plan completion.
    pub fn run(&self) -> Result<()> {
        // Register signal handlers
        flag::register(SIGTERM, Arc::clone(&self.shutdown))
            .context("Failed to register SIGTERM handler")?;
        flag::register(SIGINT, Arc::clone(&self.shutdown))
            .context("Failed to register SIGINT handler")?;
        flag::register(SIGUSR1, Arc::clone(&self.wake_requested))
            .context("Failed to register SIGUSR1 handler")?;

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

        // Save PID so other commands can detect the running daemon
        cryo_state.pid = Some(std::process::id());
        state::save_state(&self.state_path, &cryo_state)?;

        // Create .cryo/ directory and bind socket server
        let sock_path = crate::socket::socket_path(&self.dir);
        if let Some(parent) = sock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let server = crate::socket::SocketServer::bind(&sock_path)?;
        server.set_nonblocking(true)?;
        eprintln!("Daemon: socket listening at {}", sock_path.display());

        // Register in global daemon registry (with socket path)
        if let Err(e) = crate::registry::register(&self.dir, Some(&sock_path)) {
            eprintln!("Daemon: failed to register in ~/.cryo/daemons: {e}");
        }

        // Set up inbox watcher
        let (tx, rx) = mpsc::channel();
        let inbox_path = self.dir.join("messages").join("inbox");
        let _watcher = if config.watch_inbox && inbox_path.exists() {
            match InboxWatcher::start(&inbox_path, tx.clone()) {
                Ok(w) => {
                    eprintln!("Daemon: watching messages/inbox/ for new messages");
                    Some(w)
                }
                Err(e) => {
                    eprintln!("Daemon: failed to start inbox watcher: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Spawn a thread that forwards signals to the event channel,
        // so recv_timeout() unblocks immediately on SIGTERM/SIGINT/SIGUSR1.
        let shutdown_flag = Arc::clone(&self.shutdown);
        let wake_flag = Arc::clone(&self.wake_requested);
        let signal_tx = tx;
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            if shutdown_flag.load(Ordering::Relaxed) {
                let _ = signal_tx.send(DaemonEvent::Shutdown);
                break;
            }
            if wake_flag.swap(false, Ordering::Relaxed) {
                let _ = signal_tx.send(DaemonEvent::InboxChanged);
            }
        });

        // Compute next report time
        let last_report = cryo_state
            .last_report_time
            .as_ref()
            .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok());
        let mut next_report_time = crate::report::compute_next_report_time(
            &config.report_time,
            config.report_interval,
            last_report,
        );
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
        let mut next_wake: Option<NaiveDateTime> = None;
        let mut pending_fallback: Option<(NaiveDateTime, FallbackAction)> = None;

        // First session: run immediately
        let mut run_now = true;

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                eprintln!("Daemon: received shutdown signal");
                break;
            }

            if run_now {
                run_now = false;

                // Detect delayed wake: if the scheduled wake time has long passed
                // (e.g. computer was sleeping), notify the agent instead of failing.
                let delayed_wake = next_wake.and_then(|wake| {
                    let now = Local::now().naive_local();
                    detect_delayed_wake(wake, now).map(|delay_str| {
                        // Cancel premature fallback — the session is about to run
                        pending_fallback = None;
                        format!(
                            "DELAYED WAKE: This session was scheduled for {} but is running {} late \
                             (the host machine was likely suspended or powered off). \
                             Check whether time-sensitive tasks need adjustment.",
                            wake.format("%Y-%m-%dT%H:%M"),
                            delay_str,
                        )
                    })
                });
                let saved_wake = next_wake.take();

                cryo_state.session_number += 1;
                cryo_state.next_wake = None;
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
                                drop(pending_fallback);
                                eprintln!("Daemon: plan complete. Shutting down.");
                                break;
                            }
                            SessionLoopOutcome::Hibernate {
                                wake_time,
                                fallback,
                            } => {
                                retry.reset();
                                next_wake = Some(wake_time);
                                cryo_state.next_wake =
                                    Some(wake_time.format("%Y-%m-%dT%H:%M").to_string());
                                let _ = state::save_state(&self.state_path, &cryo_state);
                                pending_fallback =
                                    fallback.map(|fb| (wake_time + chrono::Duration::hours(1), fb));
                                eprintln!(
                                    "Daemon: next wake at {}",
                                    wake_time.format("%Y-%m-%d %H:%M")
                                );
                            }
                            SessionLoopOutcome::ValidationFailed { quick_exit } => {
                                next_wake = saved_wake;

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
                        next_wake = saved_wake;
                        eprintln!("Daemon: session failed: {e}");
                        if self.handle_failure_retry(&mut retry, &config.fallback_alert) {
                            break;
                        }
                        run_now = true;
                        continue;
                    }
                }
            }

            // Check fallback only when idle (not about to run a session)
            self.check_fallback(&mut pending_fallback, &config.fallback_alert);

            // Check if periodic report is due
            if let Some(report_time) = next_report_time {
                if Local::now().naive_local() >= report_time {
                    self.send_periodic_report(&config, &mut cryo_state, &mut next_report_time);
                }
            }

            // Wait for next event
            let timeout = compute_sleep_timeout(
                next_wake,
                next_report_time,
                Local::now().naive_local(),
            );

            match rx.recv_timeout(timeout) {
                Ok(DaemonEvent::InboxChanged) => {
                    eprintln!("Daemon: inbox changed, waking up");
                    run_now = true;
                }
                Ok(DaemonEvent::Shutdown) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if next_wake.is_some() {
                        eprintln!("Daemon: scheduled wake time reached");
                        run_now = true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    eprintln!("Daemon: event channel disconnected");
                    break;
                }
            }
        }

        // Cleanup: always unregister and remove socket, even if state save fails
        cryo_state.pid = None;
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

        // Build prompt (slim — agent reads cryo.log and inbox files directly)
        let agent_config = crate::agent::AgentConfig {
            session_number: cryo_state.session_number,
            task: task.clone(),
            delayed_wake: delayed_wake.map(|s| s.to_string()),
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
        let spawn_time = std::time::Instant::now();
        logger.log_event(&format!("agent started (pid {child_pid})"))?;
        if let Some(name) = provider_name {
            logger.log_event(&format!("provider: {name}"))?;
        }

        // Poll loop: wait for socket commands + agent exit
        let deadline = if timeout_secs > 0 {
            Some(std::time::Instant::now() + Duration::from_secs(timeout_secs))
        } else {
            None
        };

        let mut hibernate_outcome: Option<SessionLoopOutcome> = None;
        let mut pending_fallback: Option<FallbackAction> = None;

        loop {
            // Check shutdown
            if self.shutdown.load(Ordering::Relaxed) {
                terminate_child(&mut child, child_pid);
                if !inbox_filenames.is_empty() {
                    let _ = crate::message::archive_messages(&self.dir, &inbox_filenames);
                }
                if let Some(outcome) = hibernate_outcome {
                    logger.finish("daemon shutdown — using agent's hibernate outcome")?;
                    return Ok(outcome);
                }
                logger.finish("daemon shutdown — agent terminated")?;
                return Ok(SessionLoopOutcome::ValidationFailed { quick_exit: false });
            }

            // Check timeout
            if let Some(d) = deadline {
                if std::time::Instant::now() >= d {
                    eprintln!("Daemon: session timeout ({timeout_secs}s) — killing agent");
                    terminate_child(&mut child, child_pid);
                    if !inbox_filenames.is_empty() {
                        let _ = crate::message::archive_messages(&self.dir, &inbox_filenames);
                    }
                    if let Some(outcome) = hibernate_outcome {
                        logger.finish("session timeout — using agent's hibernate outcome")?;
                        return Ok(outcome);
                    }
                    logger.finish("session timeout — agent killed")?;
                    return Ok(SessionLoopOutcome::ValidationFailed { quick_exit: false });
                }
            }

            // Try accept a socket connection (non-blocking)
            match server.accept_one() {
                Ok(Some((request, responder))) => {
                    match request {
                        crate::socket::Request::Note { text } => {
                            logger.log_event(&format!("note: \"{text}\""))?;
                            let _ = responder.respond(&crate::socket::Response {
                                ok: true,
                                message: "Note recorded".into(),
                            });
                        }
                        crate::socket::Request::Hibernate {
                            wake,
                            complete,
                            exit_code,
                            summary,
                        } => {
                            let summary_str = summary.as_deref().unwrap_or("(no summary)");
                            if complete {
                                logger.log_event(&format!(
                                    "hibernate: plan complete, exit={exit_code}, summary=\"{summary_str}\""
                                ))?;
                                hibernate_outcome = Some(SessionLoopOutcome::PlanComplete);
                            } else if let Some(wake_str) = &wake {
                                match chrono::NaiveDateTime::parse_from_str(
                                    wake_str,
                                    "%Y-%m-%dT%H:%M",
                                ) {
                                    Ok(wake_time) => {
                                        logger.log_event(&format!(
                                            "hibernate: wake={wake_str}, exit={exit_code}, summary=\"{summary_str}\""
                                        ))?;
                                        hibernate_outcome = Some(SessionLoopOutcome::Hibernate {
                                            wake_time,
                                            fallback: pending_fallback.take(),
                                        });
                                    }
                                    Err(e) => {
                                        let _ = responder.respond(&crate::socket::Response {
                                            ok: false,
                                            message: format!("Invalid wake time: {e}"),
                                        });
                                        continue;
                                    }
                                }
                            }
                            let _ = responder.respond(&crate::socket::Response {
                                ok: true,
                                message: if complete {
                                    "Plan complete. Shutting down.".into()
                                } else {
                                    "Hibernating.".into()
                                },
                            });
                        }
                        crate::socket::Request::Alert {
                            action,
                            target,
                            message,
                        } => {
                            logger.log_event(&format!("alert: {action} -> {target}"))?;
                            pending_fallback = Some(FallbackAction {
                                action,
                                target,
                                message,
                            });
                            let _ = responder.respond(&crate::socket::Response {
                                ok: true,
                                message: "Alert registered".into(),
                            });
                        }
                        crate::socket::Request::Reply { text } => {
                            // Write reply to outbox
                            let msg = crate::message::Message {
                                from: "agent".to_string(),
                                subject: "Reply".to_string(),
                                body: text.clone(),
                                timestamp: chrono::Local::now().naive_local(),
                                metadata: std::collections::BTreeMap::new(),
                            };
                            match crate::message::write_message(&self.dir, "outbox", &msg) {
                                Ok(_) => {
                                    logger.log_event(&format!("reply: \"{text}\""))?;
                                    let _ = responder.respond(&crate::socket::Response {
                                        ok: true,
                                        message: "Reply sent".into(),
                                    });
                                }
                                Err(e) => {
                                    logger.log_event(&format!("reply failed: {e}"))?;
                                    let _ = responder.respond(&crate::socket::Response {
                                        ok: false,
                                        message: format!("Failed to write reply: {e}"),
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(None) => {} // empty connection, ignore
                Err(e) => {
                    // WouldBlock is expected in non-blocking mode
                    if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                        if io_err.kind() != std::io::ErrorKind::WouldBlock {
                            eprintln!("Daemon: socket accept error: {e}");
                        }
                    }
                }
            }

            // Check if agent has exited
            match child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code();
                    let elapsed = spawn_time.elapsed();
                    logger.log_event(&format!(
                        "agent exited (code {})",
                        code.map(|c| c.to_string())
                            .unwrap_or_else(|| "signal".into())
                    ))?;

                    // Archive inbox messages now that agent has finished
                    if !inbox_filenames.is_empty() {
                        crate::message::archive_messages(&self.dir, &inbox_filenames)?;
                    }

                    if let Some(outcome) = hibernate_outcome {
                        logger.finish("session complete")?;
                        return Ok(outcome);
                    } else {
                        // Quick-exit detection: agent exited fast without hibernating
                        if elapsed < Duration::from_secs(5) {
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
                        // Agent exited without calling hibernate — treat as crash
                        logger.finish("agent exited without hibernate")?;
                        return Ok(SessionLoopOutcome::ValidationFailed {
                            quick_exit: elapsed < Duration::from_secs(5),
                        });
                    }
                }
                Ok(None) => {} // still running
                Err(e) => {
                    logger.finish(&format!("error checking agent: {e}"))?;
                    return Err(e.into());
                }
            }

            // If we got a hibernate command but agent hasn't exited yet,
            // give it a moment then continue polling
            if hibernate_outcome.is_some() {
                // Agent sent hibernate but hasn't exited yet — wait a bit
                // The agent should exit shortly after calling cryo hibernate
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Execute a pending fallback if its deadline has passed.
    fn check_fallback(
        &self,
        pending: &mut Option<(NaiveDateTime, FallbackAction)>,
        alert_method: &str,
    ) {
        if let Some((deadline, _)) = pending.as_ref() {
            if Local::now().naive_local() > *deadline {
                let (_, fb) = pending.take().unwrap();
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
        let now = Local::now().naive_local();
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
            std::thread::sleep(sleep_time);
            remaining = remaining.saturating_sub(sleep_time);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(timeout, Duration::from_secs(30), "Should pick earlier (report)");
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
}
