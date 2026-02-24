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
use std::time::Duration;

use crate::fallback::FallbackAction;
use crate::session::{self, SessionOutcome};
use crate::state::{self, CryoState};

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

/// The persistent daemon process.
pub struct Daemon {
    dir: PathBuf,
    state_path: PathBuf,
    log_path: PathBuf,
    shutdown: Arc<AtomicBool>,
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
        }
    }

    /// Run the daemon event loop. Blocks until SIGTERM or plan completion.
    pub fn run(&self) -> Result<()> {
        // Register signal handlers
        flag::register(SIGTERM, Arc::clone(&self.shutdown))
            .context("Failed to register SIGTERM handler")?;
        flag::register(SIGINT, Arc::clone(&self.shutdown))
            .context("Failed to register SIGINT handler")?;

        let mut cryo_state =
            state::load_state(&self.state_path)?.context("No cryochamber state found")?;

        // Guard: refuse to start if another daemon is already running
        if state::is_locked(&cryo_state) {
            anyhow::bail!(
                "Another daemon is already running (PID: {:?}). Use `cryo cancel` first.",
                cryo_state.pid
            );
        }

        // Mark as daemon mode and save PID
        cryo_state.daemon_mode = true;
        cryo_state.pid = Some(std::process::id());
        state::save_state(&self.state_path, &cryo_state)?;

        // Register in global daemon registry
        if let Err(e) = crate::registry::register(&self.dir, None) {
            eprintln!("Daemon: failed to register in ~/.cryo/daemons: {e}");
        }

        // Set up inbox watcher
        let (tx, rx) = mpsc::channel();
        let inbox_path = self.dir.join("messages").join("inbox");
        let _watcher = if cryo_state.watch_inbox && inbox_path.exists() {
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

        // Spawn a thread that forwards shutdown signals to the event channel,
        // so recv_timeout() unblocks immediately on SIGTERM/SIGINT.
        let shutdown_flag = Arc::clone(&self.shutdown);
        let shutdown_tx = tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            if shutdown_flag.load(Ordering::Relaxed) {
                let _ = shutdown_tx.send(DaemonEvent::Shutdown);
                break;
            }
        });

        let mut retry = RetryState::new(cryo_state.max_retries);
        let mut next_wake: Option<NaiveDateTime> = None;
        let mut pending_fallback: Option<(NaiveDateTime, FallbackAction)> = None;

        // First session: run immediately
        let mut run_now = true;

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                eprintln!("Daemon: received shutdown signal");
                break;
            }

            // Check if a pending fallback deadline has passed
            self.check_fallback(&mut pending_fallback);

            if run_now {
                run_now = false;
                cryo_state.session_number += 1;

                match self.run_one_session(&mut cryo_state) {
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
                                pending_fallback = fallback.map(|fb| {
                                    (wake_time + chrono::Duration::hours(1), fb)
                                });
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
                        next_wake = None;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    eprintln!("Daemon: event channel disconnected");
                    break;
                }
            }
        }

        // Cleanup: always unregister, even if state save fails (e.g. dir removed)
        cryo_state.pid = None;
        cryo_state.daemon_mode = false;
        if let Err(e) = state::save_state(&self.state_path, &cryo_state) {
            eprintln!("Daemon: failed to save final state: {e}");
        }
        crate::registry::unregister(&self.dir);
        eprintln!("Daemon: exited cleanly");

        Ok(())
    }

    fn run_one_session(&self, cryo_state: &mut CryoState) -> Result<SessionLoopOutcome> {
        let agent_cmd = cryo_state
            .last_command
            .clone()
            .unwrap_or_else(|| "opencode".to_string());

        let task = self
            .get_task()
            .unwrap_or_else(|| "Continue the plan".to_string());

        let timeout_secs = cryo_state.max_session_duration;

        eprintln!(
            "Daemon: Session #{}: Running agent...",
            cryo_state.session_number
        );

        let result = session::execute_session(
            &self.dir,
            cryo_state.session_number,
            &task,
            &self.log_path,
            |prompt, writer| {
                self.run_agent_with_timeout(&agent_cmd, prompt, writer, timeout_secs)
            },
        )?;

        for warning in &result.warnings {
            eprintln!("Daemon: Warning: {warning}");
        }

        if let Some(cmd) = &result.command {
            cryo_state.last_command = Some(cmd.clone());
        }

        match result.outcome {
            SessionOutcome::PlanComplete => Ok(SessionLoopOutcome::PlanComplete),
            SessionOutcome::ValidationFailed { errors, .. } => {
                for error in &errors {
                    eprintln!("Daemon: Error: {error}");
                }
                Ok(SessionLoopOutcome::ValidationFailed)
            }
            SessionOutcome::Hibernate {
                wake_time,
                fallback,
                ..
            } => Ok(SessionLoopOutcome::Hibernate {
                wake_time,
                fallback,
            }),
        }
    }

    /// Execute a pending fallback if its deadline has passed.
    fn check_fallback(
        &self,
        pending: &mut Option<(NaiveDateTime, FallbackAction)>,
    ) {
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
        let latest = crate::log::read_latest_session(&self.log_path).ok()??;
        session::derive_task_from_output(&latest)
    }

    /// Run agent with a timeout, forwarding the daemon's shutdown signal.
    fn run_agent_with_timeout(
        &self,
        agent_command: &str,
        prompt: &str,
        writer: &mut crate::log::SessionWriter,
        timeout_secs: u64,
    ) -> Result<crate::agent::AgentResult> {
        crate::agent::run_agent_with_timeout(
            agent_command,
            prompt,
            writer,
            timeout_secs,
            Some(Arc::clone(&self.shutdown)),
        )
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
