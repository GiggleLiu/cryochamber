use std::path::{Path, PathBuf};

use super::{SessionEffects, SessionLoopOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TodoRequest {
    Add { text: String, at: String },
    Done { id: u32 },
    Remove { id: u32 },
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DaemonRequest {
    Ping,
    Hello {
        protocol_version: u32,
    },
    Hibernate {
        complete: bool,
        exit_code: u8,
        summary: Option<String>,
    },
    Reply {
        text: String,
    },
    Todo(TodoRequest),
    Receive,
}

impl From<crate::socket::Request> for DaemonRequest {
    fn from(request: crate::socket::Request) -> Self {
        match request {
            crate::socket::Request::Ping => Self::Ping,
            crate::socket::Request::Hello { protocol_version } => Self::Hello { protocol_version },
            crate::socket::Request::Hibernate {
                complete,
                exit_code,
                summary,
            } => Self::Hibernate {
                complete,
                exit_code,
                summary,
            },
            crate::socket::Request::Reply { text } => Self::Reply { text },
            crate::socket::Request::TodoAdd { text, at } => {
                Self::Todo(TodoRequest::Add { text, at })
            }
            crate::socket::Request::TodoDone { id } => Self::Todo(TodoRequest::Done { id }),
            crate::socket::Request::TodoRemove { id } => Self::Todo(TodoRequest::Remove { id }),
            crate::socket::Request::TodoList => Self::Todo(TodoRequest::List),
            crate::socket::Request::Receive => Self::Receive,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HibernateDecision {
    /// `Some` terminates the session with this outcome; `None` rejects the
    /// hibernate attempt and leaves the session running so the agent can
    /// observe the error and correct itself (e.g. register a TODO).
    pub(super) outcome: Option<SessionLoopOutcome>,
    pub(super) response_ok: bool,
    pub(super) response_message: &'static str,
    pub(super) log_event: String,
}

pub(super) fn resolve_hibernate_request(
    complete: bool,
    exit_code: u8,
    summary: Option<&str>,
    has_pending_todos: bool,
) -> HibernateDecision {
    let summary = summary.unwrap_or("(no summary)");
    if exit_code != 0 {
        return HibernateDecision {
            outcome: Some(SessionLoopOutcome::ValidationFailed { quick_exit: false }),
            response_ok: true,
            response_message: "Failure recorded. Daemon will retry.",
            log_event: format!("hibernate failed: exit={exit_code}, summary=\"{summary}\""),
        };
    }

    if complete {
        return HibernateDecision {
            outcome: Some(SessionLoopOutcome::PlanComplete),
            response_ok: true,
            response_message: "Plan complete. Shutting down.",
            log_event: format!("hibernate: plan complete, exit={exit_code}, summary=\"{summary}\""),
        };
    }

    if !has_pending_todos {
        return HibernateDecision {
            outcome: None,
            response_ok: false,
            response_message:
                "hibernate refused: no pending TODO with a valid `--at` time. Every session \
                 must declare its next wake before hibernating. Run \
                 `cryo-agent todo add \"<next step>\" --at <TIME>` (use `cryo-agent time \"+30 minutes\"` \
                 to compute TIME), then retry `cryo-agent hibernate`. Use `cryo-agent hibernate --complete` \
                 only if the plan is genuinely finished.",
            log_event: format!("hibernate refused: no pending TODO, summary=\"{summary}\""),
        };
    }

    HibernateDecision {
        outcome: Some(SessionLoopOutcome::Hibernate),
        response_ok: true,
        response_message: "Hibernating.",
        log_event: format!("hibernate: exit={exit_code}, summary=\"{summary}\""),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TodoRequestOutcome {
    pub(super) ok: bool,
    pub(super) message: String,
    pub(super) log_event: Option<String>,
}

impl TodoRequestOutcome {
    pub(super) fn into_response(self) -> crate::socket::Response {
        crate::socket::Response {
            ok: self.ok,
            message: self.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TodoOperationError {
    response_message: String,
}

impl TodoOperationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            response_message: message.into(),
        }
    }
}

pub(super) trait TodoEffects {
    fn add_todo(&mut self, text: &str, at: &str) -> std::result::Result<u32, TodoOperationError>;
    fn done_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError>;
    fn remove_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError>;
    fn list_todos(&mut self) -> std::result::Result<String, TodoOperationError>;
}

impl<T: SessionEffects> TodoEffects for T {
    fn add_todo(&mut self, text: &str, at: &str) -> std::result::Result<u32, TodoOperationError> {
        SessionEffects::todo_add(self, text, at)
            .map_err(|e| TodoOperationError::new(format!("Failed to add todo: {e}")))
    }

    fn done_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError> {
        SessionEffects::todo_done(self, id).map_err(|e| TodoOperationError::new(format!("{e}")))
    }

    fn remove_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError> {
        SessionEffects::todo_remove(self, id).map_err(|e| TodoOperationError::new(format!("{e}")))
    }

    fn list_todos(&mut self) -> std::result::Result<String, TodoOperationError> {
        SessionEffects::todo_list(self)
            .map_err(|e| TodoOperationError::new(format!("Failed to load todo list: {e}")))
    }
}

pub(super) struct FileTodoEffects {
    todo_path: PathBuf,
}

impl FileTodoEffects {
    pub(super) fn new(dir: &Path) -> Self {
        Self {
            todo_path: dir.join("todo.json"),
        }
    }

    fn load(&self) -> std::result::Result<crate::todo::TodoList, TodoOperationError> {
        crate::todo::TodoList::load(&self.todo_path)
            .map_err(|e| TodoOperationError::new(format!("Failed to load todo list: {e}")))
    }

    fn save(&self, list: &crate::todo::TodoList) -> std::result::Result<(), TodoOperationError> {
        list.save(&self.todo_path)
            .map_err(|e| TodoOperationError::new(format!("Failed to save todo: {e}")))
    }
}

impl TodoEffects for FileTodoEffects {
    fn add_todo(&mut self, text: &str, at: &str) -> std::result::Result<u32, TodoOperationError> {
        let mut list = self.load()?;
        let id = list.add(text.to_string(), at.to_string());
        self.save(&list)?;
        Ok(id)
    }

    fn done_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError> {
        let mut list = self.load()?;
        list.done(id)
            .map_err(|e| TodoOperationError::new(format!("{e}")))?;
        self.save(&list)
    }

    fn remove_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError> {
        let mut list = self.load()?;
        list.remove(id)
            .map_err(|e| TodoOperationError::new(format!("{e}")))?;
        self.save(&list)
    }

    fn list_todos(&mut self) -> std::result::Result<String, TodoOperationError> {
        Ok(self.load()?.display())
    }
}

pub(super) fn handle_todo_request(
    request: TodoRequest,
    effects: &mut impl TodoEffects,
) -> TodoRequestOutcome {
    match request {
        TodoRequest::Add { text, at } => match effects.add_todo(&text, &at) {
            Ok(id) => TodoRequestOutcome {
                ok: true,
                message: format!("Added todo #{id}"),
                log_event: Some(format!("todo add: #{id} \"{text}\" at {at}")),
            },
            Err(e) => TodoRequestOutcome {
                ok: false,
                message: e.response_message,
                log_event: None,
            },
        },
        TodoRequest::Done { id } => match effects.done_todo(id) {
            Ok(()) => TodoRequestOutcome {
                ok: true,
                message: format!("Marked todo #{id} as done"),
                log_event: Some(format!("todo done: #{id}")),
            },
            Err(e) => TodoRequestOutcome {
                ok: false,
                message: e.response_message,
                log_event: None,
            },
        },
        TodoRequest::Remove { id } => match effects.remove_todo(id) {
            Ok(()) => TodoRequestOutcome {
                ok: true,
                message: format!("Removed todo #{id}"),
                log_event: Some(format!("todo remove: #{id}")),
            },
            Err(e) => TodoRequestOutcome {
                ok: false,
                message: e.response_message,
                log_event: None,
            },
        },
        TodoRequest::List => match effects.list_todos() {
            Ok(display) => TodoRequestOutcome {
                ok: true,
                message: display,
                log_event: None,
            },
            Err(e) => TodoRequestOutcome {
                ok: false,
                message: e.response_message,
                log_event: None,
            },
        },
    }
}
