use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::config::CryoConfig;
use crate::process::send_signal_group;
use crate::state::CryoState;

use super::effects::FsSessionEffects;
use super::{session_prompt_notice, ActiveSessionContext, Clock, Daemon, SessionLoopOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChildExitStatus {
    pub(super) code: Option<i32>,
}

pub(super) trait SessionRuntime {
    fn accept_request(
        &mut self,
        expected_instance_id: Option<&str>,
    ) -> Result<Option<crate::socket::Request>>;
    fn respond(&mut self, ok: bool, message: String) -> Result<()>;
    /// Move the pending responder into a parked slot without responding.
    /// The parked client stays blocked until `respond_parked`.
    fn park(&mut self) -> Result<()>;
    /// Respond from the parked slot and clear it.
    fn respond_parked(&mut self, ok: bool, message: String) -> Result<()>;
    fn try_wait(&mut self) -> std::io::Result<Option<ChildExitStatus>>;
    fn terminate(&mut self);
}

struct ProcessSessionRuntime<'a> {
    server: &'a crate::socket::SocketServer,
    child: &'a mut std::process::Child,
    clock: Arc<dyn Clock>,
    pending_responder: Option<crate::socket::Responder>,
    parked_responder: Option<crate::socket::Responder>,
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
            parked_responder: None,
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

    fn park(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.parked_responder.is_none(),
            "a wait is already parked for this session"
        );
        let responder = self
            .pending_responder
            .take()
            .context("Missing pending session responder to park")?;
        self.parked_responder = Some(responder);
        Ok(())
    }

    fn respond_parked(&mut self, ok: bool, message: String) -> Result<()> {
        let responder = self
            .parked_responder
            .take()
            .context("Missing parked responder")?;
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

/// Materializes a single agent session.
///
/// Production code spawns a real child via [`ProcessSessionLauncher`]; tests
/// can swap in a scripted implementation that bypasses process creation and
/// drives the session purely through the injected `Clock` and event source.
/// This is what makes multi-session behavior (wake -> run -> hibernate -> sleep
/// -> wake) testable in-process without wall-clock delays.
pub(super) trait SessionLauncher: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    fn run_session(
        &self,
        daemon: &Daemon,
        config: &CryoConfig,
        cryo_state: &CryoState,
        server: &crate::socket::SocketServer,
        delayed_wake: Option<&str>,
        wake_sources: &[PathBuf],
        provider_env: &std::collections::HashMap<String, String>,
        provider_name: Option<&str>,
    ) -> Result<SessionLoopOutcome>;
}

/// Production `SessionLauncher`: spawns a real agent subprocess, wraps it in
/// `ProcessSessionRuntime`, and delegates to `Daemon::drive_active_session`.
pub(super) struct ProcessSessionLauncher;

impl SessionLauncher for ProcessSessionLauncher {
    #[allow(clippy::too_many_arguments)]
    fn run_session(
        &self,
        daemon: &Daemon,
        config: &CryoConfig,
        cryo_state: &CryoState,
        server: &crate::socket::SocketServer,
        delayed_wake: Option<&str>,
        wake_sources: &[PathBuf],
        provider_env: &std::collections::HashMap<String, String>,
        provider_name: Option<&str>,
    ) -> Result<SessionLoopOutcome> {
        let agent_cmd = config.agent.clone();

        let task = daemon
            .get_task()
            .unwrap_or_else(|| "Continue the plan".to_string());

        let timeout_secs = config.max_session_duration;

        eprintln!(
            "Daemon: Session #{}: Running agent...",
            cryo_state.session_number
        );

        let store = crate::channel::store::MessageStore::new(daemon.dir.clone());

        // Inbox messages are not shown inline anymore. The daemon still
        // detects whether any are waiting so it can tell the agent to run
        // `cryo-agent receive`, but the content itself stays hidden until
        // the agent explicitly asks for it.
        let inbox_filenames: Vec<String> = store.list_inbox_filenames()?;
        let inbox_waiting = !inbox_filenames.is_empty();
        let mut inbox_sources = format_wake_sources(&daemon.dir, wake_sources);
        if inbox_waiting && inbox_sources.is_empty() {
            inbox_sources.push(display_source_path(
                &daemon.dir,
                &daemon.dir.join("messages").join("inbox"),
            ));
        }

        let todo_path = daemon.dir.join("todo.json");
        let todo_display = match crate::todo::TodoFile::new(&todo_path).display() {
            Ok(display) => display,
            Err(err) => {
                eprintln!(
                    "Daemon: Error loading TODO list from {}: {}",
                    todo_path.display(),
                    err
                );
                format!("Error loading TODO list ({err}). Please check todo.json.")
            }
        };

        let notice = session_prompt_notice(delayed_wake, cryo_state.previous_session_crashed);

        let agent_config = crate::agent::AgentConfig {
            session_number: cryo_state.session_number,
            task: task.clone(),
            delayed_wake: notice,
            todo_list: todo_display,
            inbox_waiting,
            inbox_sources,
        };
        let prompt = crate::agent::build_prompt(&agent_config);

        let mut logger = crate::log::EventLogger::begin(
            &daemon.log_path,
            cryo_state.session_number,
            &task,
            &agent_cmd,
            &inbox_filenames,
        )?;

        if let Some(notice) = delayed_wake {
            logger.log_event(&format!("delayed wake: {notice}"))?;
        }
        if cryo_state.previous_session_crashed {
            logger.log_event("previous session crashed — agent advised to check inbox archive")?;
        }

        let agent_log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(crate::log::agent_log_path(&daemon.dir))?;

        let mut child =
            crate::agent::spawn_agent(&agent_cmd, &prompt, Some(agent_log_file), provider_env)?;
        let child_pid = child.id();
        let spawn_time = daemon.clock.monotonic_now();
        logger.log_event(&format!("agent started (pid {child_pid})"))?;
        if let Some(name) = provider_name {
            logger.log_event(&format!("provider: {name}"))?;
        }

        let mut runtime = ProcessSessionRuntime::new(server, &mut child, Arc::clone(&daemon.clock));
        let mut effects = FsSessionEffects::new(&daemon.dir);
        let context = ActiveSessionContext {
            cryo_state,
            timeout_secs,
            wait_timeout_secs: config
                .wait_timeout
                .unwrap_or(crate::config::DEFAULT_WAIT_TIMEOUT_SECS),
            spawn_time,
        };
        let outcome = daemon.drive_active_session(&mut runtime, &mut effects, context, logger);

        // `runtime` borrows `&mut child`; release the borrow before touching
        // `child` again.
        drop(runtime);

        // On the error path (e.g. an IPC respond EPIPE or a disk-full
        // `log_event`), `drive_active_session` returns `Err` without having
        // terminated the agent. Kill and reap the whole process group here so
        // the daemon never spawns a retry session alongside a still-running
        // agent. The success and shutdown/timeout paths already reaped the
        // child, so `terminate_child` short-circuits on those.
        if outcome.is_err() {
            terminate_child(&mut child, child_pid, daemon.clock.as_ref());
        }
        outcome
    }
}

fn format_wake_sources(chamber_dir: &Path, wake_sources: &[PathBuf]) -> Vec<String> {
    let mut formatted = Vec::new();
    for source in wake_sources {
        let display = display_source_path(chamber_dir, source);
        if !formatted.iter().any(|existing| existing == &display) {
            formatted.push(display);
        }
    }
    formatted
}

fn display_source_path(chamber_dir: &Path, source: &Path) -> String {
    let root = chamber_dir
        .canonicalize()
        .unwrap_or_else(|_| chamber_dir.to_path_buf());
    let path = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());

    path.strip_prefix(&root)
        .unwrap_or(&path)
        .display()
        .to_string()
}

/// Gracefully terminate a child's process group: SIGTERM, wait 2s, SIGKILL if
/// needed, then reap. The agent is spawned as its own group leader (see
/// `spawn_agent`), so `pid == pgid` and signaling the group reaches the whole
/// agent subprocess tree, not just the direct child.
fn terminate_child(child: &mut std::process::Child, pid: u32, clock: &dyn Clock) {
    // Already exited (e.g. reaped by an earlier terminate on the
    // shutdown/timeout path, or a normal self-exit): reap and return without
    // signaling — the pid could otherwise have been recycled.
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    send_signal_group(pid, libc::SIGTERM);
    clock.sleep(Duration::from_secs(2));
    if child.try_wait().ok().flatten().is_none() {
        send_signal_group(pid, libc::SIGKILL);
    }
    let _ = child.wait();
}
