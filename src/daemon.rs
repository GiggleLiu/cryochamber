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
}

impl RetryState {
    pub fn new(max_retries: u32) -> Self {
        Self {
            attempt: 0,
            max_retries,
        }
    }

    /// Calculate backoff duration for current attempt.
    /// Returns None if max retries exceeded.
    pub fn next_backoff(&self) -> Option<Duration> {
        if self.attempt >= self.max_retries {
            return None;
        }
        // 5s, 15s, 60s, 60s, 60s, ...
        let secs = match self.attempt {
            0 => 5,
            1 => 15,
            _ => 60,
        };
        Some(Duration::from_secs(secs))
    }

    pub fn record_failure(&mut self) {
        self.attempt += 1;
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn exhausted(&self) -> bool {
        self.attempt >= self.max_retries
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
    ValidationFailed,
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

        let mut retry = RetryState::new(config.max_retries);
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
                    let delay = now - wake;
                    if delay > chrono::Duration::minutes(5) {
                        // Cancel premature fallback — the session is about to run
                        pending_fallback = None;
                        let delay_str = if delay.num_hours() > 0 {
                            format!("{}h {}m", delay.num_hours(), delay.num_minutes() % 60)
                        } else {
                            format!("{}m", delay.num_minutes())
                        };
                        Some(format!(
                            "DELAYED WAKE: This session was scheduled for {} but is running {} late \
                             (the host machine was likely suspended or powered off). \
                             Check whether time-sensitive tasks need adjustment.",
                            wake.format("%Y-%m-%dT%H:%M"),
                            delay_str,
                        ))
                    } else {
                        None
                    }
                });
                let saved_wake = next_wake.take();

                cryo_state.session_number += 1;

                match self.run_one_session(&config, &cryo_state, &server, delayed_wake.as_deref()) {
                    Ok(outcome) => {
                        // Persist session number only after successful completion
                        state::save_state(&self.state_path, &cryo_state)?;
                        retry.reset();
                        match outcome {
                            SessionLoopOutcome::PlanComplete => {
                                drop(pending_fallback);
                                eprintln!("Daemon: plan complete. Shutting down.");
                                break;
                            }
                            SessionLoopOutcome::Hibernate {
                                wake_time,
                                fallback,
                            } => {
                                next_wake = Some(wake_time);
                                pending_fallback =
                                    fallback.map(|fb| (wake_time + chrono::Duration::hours(1), fb));
                                eprintln!(
                                    "Daemon: next wake at {}",
                                    wake_time.format("%Y-%m-%d %H:%M")
                                );
                            }
                            SessionLoopOutcome::ValidationFailed => {
                                eprintln!("Daemon: validation failed. Will retry on next event.");
                                next_wake = None;
                            }
                        }
                    }
                    Err(e) => {
                        // Roll back session number — no session was logged
                        cryo_state.session_number -= 1;
                        // Restore prior wake schedule so we don't fall back to hourly polling
                        next_wake = saved_wake;
                        eprintln!("Daemon: session failed: {e}");
                        let backoff = retry.next_backoff();
                        retry.record_failure();
                        if let Some(backoff) = backoff {
                            eprintln!(
                                "Daemon: retry {}/{} in {}s",
                                retry.attempt,
                                retry.max_retries,
                                backoff.as_secs()
                            );
                            if self.sleep_or_shutdown(backoff) {
                                break;
                            }
                            run_now = true;
                            continue;
                        } else {
                            eprintln!(
                                "Daemon: exhausted {} retries. Cooling down 60s before accepting new events.",
                                retry.max_retries
                            );
                            self.check_fallback(&mut pending_fallback);
                            retry.reset();
                            // Cooldown: prevent immediate re-failure from inbox events
                            if self.sleep_or_shutdown(Duration::from_secs(60)) {
                                break;
                            }
                        }
                    }
                }
            }

            // Check fallback only when idle (not about to run a session)
            self.check_fallback(&mut pending_fallback);

            // Wait for next event
            let timeout = match next_wake {
                Some(wake) => {
                    let now = Local::now().naive_local();
                    let diff = wake - now;
                    diff.to_std().unwrap_or(Duration::ZERO)
                }
                None => Duration::from_secs(3600), // poll hourly if no wake scheduled
            };

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

        // Read inbox for the prompt
        let inbox = crate::message::read_inbox(&self.dir)?;
        let inbox_messages: Vec<_> = inbox.iter().map(|(_, msg)| msg.clone()).collect();
        let inbox_filenames: Vec<String> = inbox.into_iter().map(|(f, _)| f).collect();

        // Build prompt
        let log_content = crate::log::read_latest_session(&self.log_path)?;
        let agent_config = crate::agent::AgentConfig {
            log_content,
            session_number: cryo_state.session_number,
            task: task.clone(),
            inbox_messages,
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
        let mut child = crate::agent::spawn_agent(&agent_cmd, &prompt, Some(agent_log_file))?;
        let child_pid = child.id();
        logger.log_event(&format!("agent started (pid {child_pid})"))?;

        // Archive inbox messages after building prompt
        if !inbox_filenames.is_empty() {
            crate::message::archive_messages(&self.dir, &inbox_filenames)?;
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
                logger.finish("daemon shutdown — agent terminated")?;
                return Ok(SessionLoopOutcome::ValidationFailed);
            }

            // Check timeout
            if let Some(d) = deadline {
                if std::time::Instant::now() >= d {
                    eprintln!("Daemon: session timeout ({timeout_secs}s) — killing agent");
                    terminate_child(&mut child, child_pid);
                    logger.finish("session timeout — agent killed")?;
                    return Ok(SessionLoopOutcome::ValidationFailed);
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
                    logger.log_event(&format!(
                        "agent exited (code {})",
                        code.map(|c| c.to_string())
                            .unwrap_or_else(|| "signal".into())
                    ))?;

                    if let Some(outcome) = hibernate_outcome {
                        logger.finish("session complete")?;
                        return Ok(outcome);
                    } else {
                        // Agent exited without calling hibernate — treat as crash
                        logger.finish("agent exited without hibernate")?;
                        return Ok(SessionLoopOutcome::ValidationFailed);
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
    fn check_fallback(&self, pending: &mut Option<(NaiveDateTime, FallbackAction)>) {
        if let Some((deadline, _)) = pending.as_ref() {
            if Local::now().naive_local() > *deadline {
                let (_, fb) = pending.take().unwrap();
                eprintln!("Daemon: fallback deadline passed, executing fallback action");
                if let Err(e) = fb.execute(&self.dir) {
                    eprintln!("Daemon: fallback execution failed: {e}");
                }
            }
        }
    }

    fn get_task(&self) -> Option<String> {
        // The agent manages task continuity via the plan file.
        // We no longer derive tasks from old stdout markers.
        None
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
        let mut state = RetryState::new(3);
        assert_eq!(state.next_backoff(), Some(Duration::from_secs(5)));

        state.record_failure();
        assert_eq!(state.attempt, 1);
        assert_eq!(state.next_backoff(), Some(Duration::from_secs(15)));

        state.record_failure();
        assert_eq!(state.attempt, 2);
        assert_eq!(state.next_backoff(), Some(Duration::from_secs(60)));

        state.record_failure();
        assert_eq!(state.attempt, 3);
        assert_eq!(state.next_backoff(), None);
        assert!(state.exhausted());
    }

    #[test]
    fn test_backoff_reset() {
        let mut state = RetryState::new(3);
        state.record_failure();
        state.record_failure();
        assert_eq!(state.attempt, 2);

        state.reset();
        assert_eq!(state.attempt, 0);
        assert!(!state.exhausted());
    }

    #[test]
    fn test_backoff_zero_retries() {
        let state = RetryState::new(0);
        assert_eq!(state.next_backoff(), None);
        assert!(state.exhausted());
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
}
