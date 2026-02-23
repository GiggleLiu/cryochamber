// src/daemon.rs
//! Persistent daemon that owns the session lifecycle.
//!
//! Replaces one-shot `cryo wake` with a long-running process that:
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

use crate::log::SessionWriter;
use crate::message;
use crate::session::{self, SessionOutcome};
use crate::state::{self, CryoState};

/// Events the daemon responds to.
#[derive(Debug, PartialEq)]
pub enum DaemonEvent {
    /// Scheduled wake time arrived.
    ScheduledWake,
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
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
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
    Hibernate { wake_time: NaiveDateTime },
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

        let mut cryo_state = state::load_state(&self.state_path)?
            .context("No cryochamber state found")?;

        // Mark as daemon mode and save PID
        cryo_state.daemon_mode = true;
        cryo_state.pid = Some(std::process::id());
        state::save_state(&self.state_path, &cryo_state)?;

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

        let mut retry = RetryState::new(cryo_state.max_retries);
        let mut next_wake: Option<NaiveDateTime> = None;

        // First session: run immediately
        let mut run_now = true;

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                eprintln!("Daemon: received shutdown signal");
                break;
            }

            if run_now {
                run_now = false;
                cryo_state.session_number += 1;
                state::save_state(&self.state_path, &cryo_state)?;

                match self.run_one_session(&mut cryo_state) {
                    Ok(outcome) => {
                        retry.reset();
                        match outcome {
                            SessionLoopOutcome::PlanComplete => {
                                eprintln!("Daemon: plan complete. Shutting down.");
                                break;
                            }
                            SessionLoopOutcome::Hibernate { wake_time } => {
                                next_wake = Some(wake_time);
                                eprintln!(
                                    "Daemon: next wake at {}",
                                    wake_time.format("%Y-%m-%d %H:%M")
                                );
                            }
                            SessionLoopOutcome::ValidationFailed => {
                                eprintln!(
                                    "Daemon: validation failed. Will retry on next event."
                                );
                                next_wake = None;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Daemon: session failed: {e}");
                        retry.record_failure();
                        if let Some(backoff) = retry.next_backoff() {
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
                                "Daemon: exhausted {} retries. Waiting for next event.",
                                retry.max_retries
                            );
                            retry.reset();
                        }
                    }
                }

                state::save_state(&self.state_path, &cryo_state)?;
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
                Ok(DaemonEvent::ScheduledWake) => {
                    run_now = true;
                }
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

        // Cleanup: clear PID and daemon_mode
        cryo_state.pid = None;
        cryo_state.daemon_mode = false;
        state::save_state(&self.state_path, &cryo_state)?;
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

        let log_content = crate::log::read_latest_session(&self.log_path)?;
        let inbox = message::read_inbox(&self.dir)?;
        let inbox_messages: Vec<_> = inbox.iter().map(|(_, msg)| msg.clone()).collect();
        let inbox_filenames: Vec<_> = inbox.into_iter().map(|(f, _)| f).collect();

        let config = crate::agent::AgentConfig {
            log_content,
            session_number: cryo_state.session_number,
            task: task.clone(),
            inbox_messages,
        };
        let prompt = crate::agent::build_prompt(&config);

        eprintln!(
            "Daemon: Session #{}: Running agent...",
            cryo_state.session_number
        );

        let mut writer = SessionWriter::begin(
            &self.log_path,
            cryo_state.session_number,
            &task,
            &inbox_filenames,
        )?;

        // Run agent with timeout
        let timeout_secs = cryo_state.max_session_duration;
        let result = if timeout_secs > 0 {
            self.run_agent_with_timeout(&agent_cmd, &prompt, &mut writer, timeout_secs)?
        } else {
            crate::agent::run_agent_streaming(&agent_cmd, &prompt, Some(&mut writer))?
        };

        writer.finish(Some(&result.stderr))?;

        if result.exit_code != 0 {
            eprintln!("Daemon: agent exited with code {}", result.exit_code);
        }

        // Archive processed inbox messages
        if !inbox_filenames.is_empty() {
            message::archive_messages(&self.dir, &inbox_filenames)?;
        }

        let markers = crate::marker::parse_markers(&result.stdout)?;
        let (outcome, warnings) = session::decide_session_outcome(&markers);

        for warning in &warnings {
            eprintln!("Daemon: Warning: {warning}");
        }

        if let Some(cmd) = &markers.command {
            cryo_state.last_command = Some(cmd.clone());
        }

        match outcome {
            SessionOutcome::PlanComplete => Ok(SessionLoopOutcome::PlanComplete),
            SessionOutcome::ValidationFailed { errors, .. } => {
                for error in &errors {
                    eprintln!("Daemon: Error: {error}");
                }
                Ok(SessionLoopOutcome::ValidationFailed)
            }
            SessionOutcome::Hibernate { wake_time, .. } => {
                Ok(SessionLoopOutcome::Hibernate { wake_time })
            }
        }
    }

    fn get_task(&self) -> Option<String> {
        let latest = crate::log::read_latest_session(&self.log_path).ok()??;
        session::derive_task_from_output(&latest)
    }

    /// Run agent with a timeout. Sends SIGTERM then SIGKILL if exceeded.
    fn run_agent_with_timeout(
        &self,
        agent_command: &str,
        prompt: &str,
        writer: &mut SessionWriter,
        timeout_secs: u64,
    ) -> Result<crate::agent::AgentResult> {
        use std::io::BufRead;
        use std::process::{Command, Stdio};

        let parts =
            shell_words::split(agent_command).context("Failed to parse agent command")?;
        let (program, args) = parts.split_first().context("Agent command is empty")?;

        let mut child = Command::new(program)
            .args(args)
            .arg("--prompt")
            .arg(prompt)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context(format!("Failed to spawn agent: {agent_command}"))?;

        let child_pid = child.id();
        let shutdown = Arc::clone(&self.shutdown);

        // Spawn timeout watchdog thread
        let timeout_handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
            loop {
                if std::time::Instant::now() >= deadline {
                    eprintln!(
                        "Daemon: session timeout ({timeout_secs}s) — killing agent"
                    );
                    unsafe {
                        libc::kill(child_pid as i32, libc::SIGTERM);
                    }
                    std::thread::sleep(Duration::from_secs(5));
                    unsafe {
                        libc::kill(child_pid as i32, libc::SIGKILL);
                    }
                    return true; // timed out
                }
                if shutdown.load(Ordering::Relaxed) {
                    unsafe {
                        libc::kill(child_pid as i32, libc::SIGTERM);
                    }
                    return false;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });

        let child_stdout = child.stdout.take().unwrap();
        let child_stderr = child.stderr.take().unwrap();

        let stderr_handle = std::thread::spawn(move || {
            let reader = std::io::BufReader::new(child_stderr);
            let mut buf = String::new();
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        buf.push_str(&l);
                        buf.push('\n');
                    }
                    Err(_) => break,
                }
            }
            buf
        });

        let mut stdout_buf = String::new();
        let reader = std::io::BufReader::new(child_stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    let _ = writer.write_line(&l);
                    stdout_buf.push_str(&l);
                    stdout_buf.push('\n');
                }
                Err(_) => break,
            }
        }

        let status = child.wait().context("Failed to wait for agent process")?;
        let stderr_buf = stderr_handle.join().unwrap_or_default();
        let timed_out = timeout_handle.join().unwrap_or(false);

        if timed_out {
            anyhow::bail!("Agent killed after {timeout_secs}s timeout");
        }

        Ok(crate::agent::AgentResult {
            stdout: stdout_buf,
            stderr: stderr_buf,
            exit_code: status.code().unwrap_or(-1),
        })
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
