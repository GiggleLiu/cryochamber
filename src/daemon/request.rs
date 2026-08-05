use std::path::Path;

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
    Dialog {
        filter: crate::socket::DialogFilter,
    },
    Send {
        text: String,
        question: bool,
    },
    Hibernate {
        complete: bool,
        exit_code: u8,
        summary: Option<String>,
    },
    Receive,
    ReceiveWait {
        timeout_secs: Option<u64>,
    },
    Todo(TodoRequest),
}

impl From<crate::socket::Request> for DaemonRequest {
    fn from(request: crate::socket::Request) -> Self {
        match request {
            crate::socket::Request::Ping => Self::Ping,
            crate::socket::Request::Hello { protocol_version } => Self::Hello { protocol_version },
            crate::socket::Request::Dialog { filter } => Self::Dialog { filter },
            crate::socket::Request::Send { text, question } => Self::Send { text, question },
            crate::socket::Request::Hibernate {
                complete,
                exit_code,
                summary,
            } => Self::Hibernate {
                complete,
                exit_code,
                summary,
            },
            crate::socket::Request::Receive => Self::Receive,
            crate::socket::Request::ReceiveWait { timeout_secs } => {
                Self::ReceiveWait { timeout_secs }
            }
            crate::socket::Request::TodoAdd { text, at } => {
                Self::Todo(TodoRequest::Add { text, at })
            }
            crate::socket::Request::TodoDone { id } => Self::Todo(TodoRequest::Done { id }),
            crate::socket::Request::TodoRemove { id } => Self::Todo(TodoRequest::Remove { id }),
            crate::socket::Request::TodoList => Self::Todo(TodoRequest::List),
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
pub(super) struct ReceiveRequestOutcome {
    pub(super) ok: bool,
    pub(super) message: String,
    pub(super) log_event: Option<String>,
    pub(super) claimed_filenames: Vec<String>,
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

    pub(super) fn message(&self) -> &str {
        &self.response_message
    }
}

pub(super) trait TodoEffects {
    fn add_todo(&mut self, text: &str, at: &str) -> std::result::Result<u32, TodoOperationError>;
    fn done_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError>;
    fn remove_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError>;
    fn list_todos(&mut self) -> std::result::Result<String, TodoOperationError>;
}

pub(super) trait MessageEffects {
    fn claim_inbox_batch(
        &mut self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError>;

    fn read_inbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError>;

    fn read_outbox(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError>;

    fn read_outbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError>;
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

impl<T: SessionEffects> MessageEffects for T {
    fn claim_inbox_batch(
        &mut self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        SessionEffects::claim_inbox_batch(self)
            .map_err(|e| TodoOperationError::new(format!("Failed to read inbox: {e}")))
    }

    fn read_inbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        SessionEffects::read_inbox_archive(self)
            .map_err(|e| TodoOperationError::new(format!("Failed to read inbox archive: {e}")))
    }

    fn read_outbox(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        SessionEffects::read_outbox(self)
            .map_err(|e| TodoOperationError::new(format!("Failed to read outbox: {e}")))
    }

    fn read_outbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        SessionEffects::read_outbox_archive(self)
            .map_err(|e| TodoOperationError::new(format!("Failed to read outbox archive: {e}")))
    }
}

pub(super) struct FileTodoEffects {
    todo_file: crate::todo::TodoFile,
}

impl FileTodoEffects {
    pub(super) fn new(dir: &Path) -> Self {
        Self {
            todo_file: crate::todo::TodoFile::new(dir.join("todo.json")),
        }
    }
}

/// File-backed `MessageEffects` used only by tests that exercise the dialog /
/// receive handlers against a real `MessageStore`. Production idle-mode inbox
/// access is intentionally refused (see `handle_idle_request`), and the active
/// session path uses `FsSessionEffects`, so this has no production caller.
#[cfg(test)]
pub(super) struct FileMessageEffects {
    store: crate::channel::store::MessageStore,
}

#[cfg(test)]
impl FileMessageEffects {
    pub(super) fn new(dir: &Path) -> Self {
        Self {
            store: crate::channel::store::MessageStore::new(dir.to_path_buf()),
        }
    }
}

impl TodoEffects for FileTodoEffects {
    fn add_todo(&mut self, text: &str, at: &str) -> std::result::Result<u32, TodoOperationError> {
        self.todo_file
            .add(text.to_string(), at.to_string())
            .map_err(|e| TodoOperationError::new(format!("Failed to add todo: {e}")))
    }

    fn done_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError> {
        self.todo_file
            .done(id)
            .map_err(|e| TodoOperationError::new(format!("{e}")))
    }

    fn remove_todo(&mut self, id: u32) -> std::result::Result<(), TodoOperationError> {
        self.todo_file
            .remove(id)
            .map_err(|e| TodoOperationError::new(format!("{e}")))
    }

    fn list_todos(&mut self) -> std::result::Result<String, TodoOperationError> {
        self.todo_file
            .display()
            .map_err(|e| TodoOperationError::new(format!("Failed to load todo list: {e}")))
    }
}

#[cfg(test)]
impl MessageEffects for FileMessageEffects {
    fn claim_inbox_batch(
        &mut self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        self.store
            .read_and_archive_inbox()
            .map_err(|e| TodoOperationError::new(format!("Failed to read inbox: {e}")))
    }

    fn read_inbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        self.store
            .read_inbox_archive_named()
            .map_err(|e| TodoOperationError::new(format!("Failed to read inbox archive: {e}")))
    }

    fn read_outbox(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        self.store
            .read_outbox_named()
            .map_err(|e| TodoOperationError::new(format!("Failed to read outbox: {e}")))
    }

    fn read_outbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, crate::message::Message)>, TodoOperationError> {
        self.store
            .read_outbox_archive_named()
            .map_err(|e| TodoOperationError::new(format!("Failed to read outbox archive: {e}")))
    }
}

pub(super) fn handle_todo_request(
    request: TodoRequest,
    effects: &mut impl TodoEffects,
    now: chrono::NaiveDateTime,
) -> TodoRequestOutcome {
    match request {
        TodoRequest::Add { text, at } => {
            // Normalize (and thereby validate) the `at` value here so the
            // scheduler only ever sees canonical `%Y-%m-%dT%H:%M` strings.
            // Otherwise the item would be accepted but `claim_due` /
            // `next_valid_wake` would skip it forever, so the TODO is never
            // honoured.
            let at = match crate::todo::normalize_wake_input(&at, now) {
                Ok(canonical) => canonical,
                Err(e) => {
                    return TodoRequestOutcome {
                        ok: false,
                        message: format!(
                        "todo add refused: `--at` value {at:?} is not a valid scheduled time. {e}"
                    ),
                        log_event: None,
                    }
                }
            };
            match effects.add_todo(&text, &at) {
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
            }
        }
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

pub(super) fn handle_receive_request(effects: &mut impl MessageEffects) -> ReceiveRequestOutcome {
    match effects.claim_inbox_batch() {
        Ok(messages) => {
            let claimed_filenames = messages
                .iter()
                .map(|(filename, _)| filename.clone())
                .collect::<Vec<_>>();
            let log_event = if claimed_filenames.is_empty() {
                Some("receive: inbox empty".into())
            } else {
                Some(format!(
                    "receive: {} inbox message{} [{}]",
                    claimed_filenames.len(),
                    if claimed_filenames.len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    claimed_filenames.join(", "),
                ))
            };
            let message = if messages.is_empty() {
                "No messages in inbox.".to_string()
            } else {
                crate::message::format_inbox(&messages)
            };
            ReceiveRequestOutcome {
                ok: true,
                message,
                log_event,
                claimed_filenames,
            }
        }
        Err(e) => ReceiveRequestOutcome {
            ok: false,
            message: e.response_message,
            log_event: None,
            claimed_filenames: Vec::new(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DialogRequestOutcome {
    pub(super) ok: bool,
    pub(super) message: String,
    pub(super) log_event: Option<String>,
    pub(super) claimed_filenames: Vec<String>,
}

pub(super) fn handle_dialog_request(
    filter: crate::socket::DialogFilter,
    extra_session_new: &[String],
    effects: &mut impl MessageEffects,
) -> DialogRequestOutcome {
    use crate::daemon::dialog::{render_dialog, DialogInputs};

    let claimed = match effects.claim_inbox_batch() {
        Ok(claimed) => claimed,
        Err(error) => {
            return DialogRequestOutcome {
                ok: false,
                message: format!("dialog refused: {}", error.message()),
                log_event: None,
                claimed_filenames: Vec::new(),
            };
        }
    };
    let claimed_filenames = claimed
        .iter()
        .map(|(filename, _)| filename.clone())
        .collect::<Vec<_>>();

    let archived_inbox = match effects.read_inbox_archive() {
        Ok(messages) => messages,
        Err(error) => {
            return DialogRequestOutcome {
                ok: false,
                message: format!("dialog refused: {}", error.message()),
                log_event: None,
                claimed_filenames,
            };
        }
    };

    let archived_outbox = match effects.read_outbox_archive() {
        Ok(messages) => messages,
        Err(error) => {
            return DialogRequestOutcome {
                ok: false,
                message: format!("dialog refused: {}", error.message()),
                log_event: None,
                claimed_filenames,
            };
        }
    };
    let pending_outbox = match effects.read_outbox() {
        Ok(messages) => messages,
        Err(error) => {
            return DialogRequestOutcome {
                ok: false,
                message: format!("dialog refused: {}", error.message()),
                log_event: None,
                claimed_filenames,
            };
        }
    };

    let resolved_filter = match resolve_dialog_filter(filter) {
        Ok(filter) => filter,
        Err(message) => {
            return DialogRequestOutcome {
                ok: false,
                message,
                log_event: None,
                claimed_filenames,
            };
        }
    };

    let mut new_filenames = claimed_filenames.clone();
    for filename in extra_session_new {
        if !new_filenames.iter().any(|existing| existing == filename) {
            new_filenames.push(filename.clone());
        }
    }

    let mut outbox_messages = archived_outbox;
    outbox_messages.extend(pending_outbox);

    let message = render_dialog(
        &DialogInputs {
            archived_inbox,
            outbox: outbox_messages,
            new_filenames,
        },
        resolved_filter,
    );

    let log_event = if claimed_filenames.is_empty() {
        Some("dialog: no new messages claimed".to_string())
    } else {
        Some(format!(
            "dialog: claimed {} message{} [{}]",
            claimed_filenames.len(),
            if claimed_filenames.len() == 1 {
                ""
            } else {
                "s"
            },
            claimed_filenames.join(", ")
        ))
    };

    DialogRequestOutcome {
        ok: true,
        message,
        log_event,
        claimed_filenames,
    }
}

fn resolve_dialog_filter(
    filter: crate::socket::DialogFilter,
) -> std::result::Result<crate::daemon::dialog::DialogFilterResolved, String> {
    use crate::daemon::dialog::DialogFilterResolved;

    match filter {
        crate::socket::DialogFilter::All => Ok(DialogFilterResolved::All),
        crate::socket::DialogFilter::LastN { count } => {
            if count == 0 {
                Err("dialog refused: --last must be at least 1".to_string())
            } else {
                Ok(DialogFilterResolved::LastN(count))
            }
        }
        crate::socket::DialogFilter::Since { iso } => {
            let parsed = parse_dialog_since(&iso).ok_or_else(|| {
                format!(
                    "dialog refused: --since {iso:?} is not a recognized timestamp \
                     (expected YYYY-MM-DD or YYYY-MM-DDTHH:MM)"
                )
            })?;
            Ok(DialogFilterResolved::Since(parsed))
        }
    }
}

fn parse_dialog_since(input: &str) -> Option<chrono::NaiveDateTime> {
    for format in [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(input, format) {
            return Some(parsed);
        }
    }

    chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
}

/// Resolve the wait timeout for a `ReceiveWait` request: the request's value
/// if given, else the chamber default, clamped into [1, MAX_WAIT_TIMEOUT_SECS].
pub(super) fn effective_wait_timeout(requested: Option<u64>, default_secs: u64) -> u64 {
    requested
        .unwrap_or(default_secs)
        .clamp(1, crate::config::MAX_WAIT_TIMEOUT_SECS)
}
