use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;

use crate::config::CryoConfig;
use crate::process::send_signal;
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
    fn try_wait(&mut self) -> std::io::Result<Option<ChildExitStatus>>;
    fn terminate(&mut self);
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
            spawn_time,
        };
        daemon.drive_active_session(&mut runtime, &mut effects, context, logger)
    }
}

/// Gracefully terminate a child process: SIGTERM, wait 2s, SIGKILL if needed.
fn terminate_child(child: &mut std::process::Child, pid: u32, clock: &dyn Clock) {
    send_signal(pid, libc::SIGTERM);
    clock.sleep(Duration::from_secs(2));
    if child.try_wait().ok().flatten().is_none() {
        send_signal(pid, libc::SIGKILL);
    }
    let _ = child.wait();
}
