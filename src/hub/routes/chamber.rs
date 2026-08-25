//! Per-chamber HTTP handlers. All functions take `dir: &Path` so they can be
//! reused across chambers — nothing here is tied to a single global project.

use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::channel::store::MessageStore;
use crate::hub::state::{AppState, SseEvent};

/// Build the JSON status payload for a single chamber.
pub fn status_json(dir: &Path) -> Value {
    let status = crate::chamber_status::status(dir);

    json!({
        "running": status.running,
        "agent_running": status.agent_running,
        "session": status.session,
        "agent": status.agent,
        "config_agent": status.config_agent,
        "log_tail": status.log_tail,
        "daily_digests": status.daily_digests,
        "next_wake": status.next_wake,
        "notes_content": status.notes_content,
        "notes_html": status.notes_html,
        "plan_content": status.plan_content,
        "plan_html": status.plan_html,
        // `config_content` (the raw cryo.toml, which may hold an API key) is
        // deliberately never serialized. The masked `settings_rows` (env key
        // names only) plus `has_config` carry everything the UI needs.
        "has_config": status.has_config,
        "settings_rows": status.settings_rows,
        "task": status.task,
        "session_summary": status.session_summary,
        "completed": status.completed,
        "completion_summary": status.completion_summary,
    })
}

/// Build the JSON TODO list for a chamber. Items are returned in file order
/// (i.e. insertion order). Missing `todo.json` and parse errors both yield `[]`.
pub fn todos_json(dir: &Path) -> Value {
    serde_json::to_value(crate::chamber_status::todos(dir))
        .unwrap_or_else(|_| Value::Array(Vec::new()))
}

/// Build the list of all messages (archive + inbox + outbox) for a chamber.
pub fn messages_json(dir: &Path) -> Value {
    serde_json::to_value(crate::chamber_status::messages(dir))
        .unwrap_or_else(|_| Value::Array(Vec::new()))
}

pub async fn get_status(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(status_json(&path)))
}

pub async fn get_messages(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(messages_json(&path)))
}

pub async fn get_todos(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(todos_json(&path)))
}

#[derive(Deserialize)]
pub struct SendRequest {
    body: String,
    from: Option<String>,
    subject: Option<String>,
}

/// Send a message into a chamber's inbox.
///
/// In public mode the sender identity is the server's to decide, never the
/// browser's: an invite is attributed to its own name, and the owner to the
/// configured `owner_name` (default `human`). A client-supplied `from` is
/// ignored in both cases, so nobody can sign a message as somebody else.
///
/// Open (loopback) mode has no role layer at all — the local user already has
/// shell access to the chamber — so there `from` is still honored.
///
/// Answers `{"ok":true,"id":<mailbox id>}` — the id the messages list and the
/// SSE `message` frame use for the same file, so the client can reconcile an
/// optimistic bubble on it. A write failure is a `500 {"error"}`: the client
/// treats non-2xx as failure and shows the server's text, so a `200 ok:false`
/// would be a lie it has to special-case.
///
/// Argument order matters: axum requires the `Json` body extractor last.
pub async fn post_send(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    role: Option<axum::Extension<crate::hub::tokens::Role>>,
    owner_name: Option<axum::Extension<crate::hub::config::OwnerName>>,
    Json(req): Json<SendRequest>,
) -> Response {
    let Some((path, entry)) = app.resolve(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Some(throttled) = app.write_limiter.refuse(role.as_ref().map(|e| &e.0)) {
        return throttled;
    }
    let from = match role {
        Some(axum::Extension(crate::hub::tokens::Role::Invite { name, .. })) => name,
        Some(axum::Extension(crate::hub::tokens::Role::Owner)) => owner_name
            .map(|axum::Extension(crate::hub::config::OwnerName(name))| name)
            .unwrap_or_else(|| "human".into()),
        None => req.from.unwrap_or_else(|| "human".into()),
    };
    let store = MessageStore::new(path.clone());
    let msg = crate::message::Message {
        from,
        subject: req.subject.unwrap_or_default(),
        body: req.body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
        is_question: false,
    };
    match store.send_in(&msg) {
        Ok(written) => {
            let id = crate::chamber_status::message_id_for_path("inbox", &written);
            let _ = app.tx.send(SseEvent::NewMessage {
                id: id.clone(),
                chamber_id: entry.id,
                direction: "inbox".into(),
                from: msg.from.clone(),
                subject: msg.subject.clone(),
                body: msg.body.clone(),
                timestamp: msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
                is_question: msg.is_question,
            });
            (StatusCode::OK, Json(json!({"ok": true, "id": id}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed: {e}")})),
        )
            .into_response(),
    }
}

/// Cap on a plan written through the API. `plan.md` is the chamber's brief, not
/// a document store; a megabyte is far past any brief an agent can act on, and
/// the router's own body limit is sized for 25 MB attachments, so without this
/// the plan editor would inherit the attachment cap.
pub const MAX_PLAN_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
pub struct PlanRequest {
    content: String,
}

/// Replace a chamber's `plan.md`.
///
/// Owner-only. Unlike the agent setting, this needs no restart and no daemon
/// involvement: the agent is told to read `plan.md` at the top of every
/// session, so the next wake sees the new brief.
///
/// Last write wins. The chamber's own agent can rewrite `plan.md` between the
/// editor loading it and the operator saving — there is no revision token,
/// because the agent is instructed to keep its running state in `NOTES.md` and
/// the operator is the one who owns the plan.
pub async fn post_plan(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PlanRequest>,
) -> Response {
    let Some((path, _entry)) = app.resolve(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if req.content.len() > MAX_PLAN_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": format!(
                    "plan is {} bytes; the limit is {MAX_PLAN_BYTES}",
                    req.content.len()
                )
            })),
        )
            .into_response();
    }
    // Written with a trailing newline: `plan.md` is a text file every other
    // writer (the `cryo init` template, an editor on the host) ends with one,
    // and a textarea does not.
    let mut content = req.content.replace("\r\n", "\n");
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if let Err(error) = std::fs::write(path.join("plan.md"), &content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("could not write plan.md: {error}") })),
        )
            .into_response();
    }
    app.refresh();
    (StatusCode::OK, Json(json!({ "bytes": content.len() }))).into_response()
}

#[derive(Deserialize)]
pub struct AgentRequest {
    agent: String,
}

/// Set one chamber's `agent` command in its own `cryo.toml`.
///
/// Owner-only, like every other write to a chamber's working state. The command
/// is checked for parseability, not for existence on `PATH`: `cryo.toml`
/// documents "any executable on PATH", and the preflight that a runner is
/// actually installed belongs to `cryo start`, which runs on the chamber's own
/// machine.
///
/// Answers `{agent, restart_required, override_active}`. The daemon reads
/// `cryo.toml` once when it starts, so a chamber that is already running keeps
/// its old runner until it is restarted — the response says so rather than
/// restarting a live session out from under a working agent. `override_active`
/// is the case a restart would *not* fix: `cryo start --agent` parked an
/// override in `timer.json`, and `apply_overrides` makes the daemon ignore
/// `cryo.toml`'s `agent` for as long as it is there. Writing the file is still
/// the right thing to do — it is where the override is eventually dropped back
/// to — but the caller has to be told it is not what runs tonight.
pub async fn post_agent(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<AgentRequest>,
) -> Response {
    let Some((path, _entry)) = app.resolve(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let agent = req.agent.trim();
    if agent.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "agent command is empty" })),
        )
            .into_response();
    }
    // `{error:#}` — anyhow's outermost context alone is "Failed to parse agent
    // command", which tells the operator nothing about the unbalanced quote
    // that caused it.
    if let Err(error) = crate::agent::agent_program(agent) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid agent command: {error:#}") })),
        )
            .into_response();
    }

    let config_path = crate::config::config_path(&path);
    let mut config = match crate::config::load_config(&config_path) {
        Ok(config) => config.unwrap_or_default(),
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("could not read cryo.toml: {error:#}") })),
            )
                .into_response()
        }
    };
    let state = crate::state::load_state(&crate::state::state_path(&path))
        .ok()
        .flatten();
    let answer = |agent: &str| {
        json!({
            "agent": agent,
            "restart_required": state.as_ref().is_some_and(crate::state::is_locked),
            "override_active": state
                .as_ref()
                .is_some_and(|st| st.agent_override.is_some()),
        })
    };
    if config.agent == agent {
        return (StatusCode::OK, Json(answer(&config.agent))).into_response();
    }
    config.agent = agent.to_string();
    if let Err(error) = crate::config::save_config(&config_path, &config) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("could not write cryo.toml: {error:#}") })),
        )
            .into_response();
    }
    app.refresh();
    (StatusCode::OK, Json(answer(&config.agent))).into_response()
}

pub async fn post_start(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    if entry.archived {
        return Ok(Json(json!({
            "ok": false,
            "message": "Unarchive the chamber before launching it",
        })));
    }
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::start_chamber).await;
    Ok(Json(lifecycle_status_json(result, "Started")))
}

pub async fn post_stop(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::stop_chamber).await;
    Ok(Json(lifecycle_status_json(result, "Stopped")))
}

pub async fn post_restart(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::restart_chamber).await;
    Ok(Json(lifecycle_status_json(result, "Restarted")))
}

pub async fn post_reset(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    // `archive_runtime` renames `messages/` away; the existing notify handle
    // keeps watching the archived dir. Drop it so the refresh at the end of
    // `run_blocking_lifecycle` re-creates the watcher on the fresh `messages/`.
    app.watchers.drop_watcher(&path);
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::reset_chamber).await;
    match result {
        Ok(archive) => Ok(Json(json!({
            "ok": true,
            "message": format!("Reset: logs archived to {}", archive.display()),
            "archive": archive.display().to_string(),
        }))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

pub async fn post_archive(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::archive_chamber).await;
    Ok(Json(lifecycle_status_json(result, "Archived")))
}

pub async fn post_unarchive(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::unarchive_chamber).await;
    Ok(Json(lifecycle_status_json(result, "Unarchived")))
}

fn lifecycle_status_json(result: anyhow::Result<()>, success_message: &str) -> Value {
    match result {
        Ok(()) => json!({"ok": true, "message": success_message}),
        Err(e) => json!({"ok": false, "message": e.to_string()}),
    }
}

async fn run_blocking_lifecycle<F, T>(
    app: Arc<AppState>,
    path: std::path::PathBuf,
    action: F,
) -> anyhow::Result<T>
where
    F: FnOnce(&std::path::Path) -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let result = tokio::task::spawn_blocking(move || action(&path)).await;
    app.refresh();
    match result {
        Ok(result) => result,
        Err(e) => Err(anyhow::anyhow!("Lifecycle task failed: {e}")),
    }
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/chamber.rs"]
mod tests;
