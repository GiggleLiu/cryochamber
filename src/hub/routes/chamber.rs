//! Per-chamber HTTP handlers. All functions take `dir: &Path` so they can be
//! reused across chambers — nothing here is tied to a single global project.

use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::channel::store::MessageStore;
use crate::hub::state::{AppState, SseEvent};

/// Build the JSON status payload for a single chamber.
pub fn status_json(dir: &Path) -> Value {
    let status = crate::chamber_status::status(dir);
    let next_wake_rel = status.next_wake.as_deref().and_then(|w| {
        let wake = chrono::NaiveDateTime::parse_from_str(w, "%Y-%m-%dT%H:%M").ok()?;
        let now = chrono::Local::now().naive_local();
        let diff_ms = (wake - now).num_milliseconds();
        Some(format!(
            "{w} ({})",
            crate::hub::format_relative_time(diff_ms)
        ))
    });

    json!({
        "running": status.running,
        "agent_running": status.agent_running,
        "session": status.session,
        "agent": status.agent,
        "log_tail": status.log_tail,
        "next_wake": next_wake_rel,
        "notes_content": status.notes_content,
        "notes_html": status.notes_html,
        "plan_content": status.plan_content,
        "plan_html": status.plan_html,
        "config_content": status.config_content,
        "settings_rows": status.settings_rows,
        "task": status.task,
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

pub async fn post_send(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<SendRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let store = MessageStore::new(path.clone());
    let msg = crate::message::Message {
        from: req.from.unwrap_or_else(|| "human".into()),
        subject: req.subject.unwrap_or_default(),
        body: req.body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
        is_question: false,
    };
    match store.send_in(&msg) {
        Ok(_) => {
            let _ = app.tx.send(SseEvent::NewMessage {
                chamber_id: entry.id,
                direction: "inbox".into(),
                from: msg.from.clone(),
                subject: msg.subject.clone(),
                body: msg.body.clone(),
                timestamp: msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
                is_question: msg.is_question,
            });
            Ok(Json(json!({"ok": true, "message": "Message sent"})))
        }
        Err(e) => Ok(Json(
            json!({"ok": false, "message": format!("Failed: {e}")}),
        )),
    }
}

#[derive(Deserialize, Default)]
pub struct WakeRequest {
    message: Option<String>,
}

pub async fn post_wake(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<WakeRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let store = MessageStore::new(path.clone());
    let body = req
        .message
        .unwrap_or_else(|| "Wake requested from web UI.".into());
    let msg = crate::message::Message {
        from: "operator".into(),
        subject: "Wake".into(),
        body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
        is_question: false,
    };
    if let Err(e) = store.send_in(&msg) {
        return Ok(Json(
            json!({"ok": false, "message": format!("Failed: {e}")}),
        ));
    }
    let signaled = crate::daemon_client::signal_daemon_wake(&path);
    Ok(Json(json!({
        "ok": true,
        "message": wake_response_message(signaled)
    })))
}

pub async fn post_start(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
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
    let _ = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({
        "ok": false,
        "message": "Archive is disabled in the global hub"
    })))
}

fn lifecycle_status_json(result: anyhow::Result<()>, success_message: &str) -> Value {
    match result {
        Ok(()) => json!({"ok": true, "message": success_message}),
        Err(e) => json!({"ok": false, "message": e.to_string()}),
    }
}

fn wake_response_message(signaled: bool) -> &'static str {
    match signaled {
        true => "Wake signal sent",
        false => "Message queued (no daemon running)",
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
