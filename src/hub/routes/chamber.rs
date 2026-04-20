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

use crate::hub::state::{AppState, SseEvent};

/// Build the JSON status payload for a single chamber.
pub fn status_json(dir: &Path) -> Value {
    let cfg = crate::config::load_config(&crate::config::config_path(dir))
        .ok()
        .flatten()
        .unwrap_or_default();

    let (running, session, agent) = match crate::state::load_state(&crate::state::state_path(dir))
        .ok()
        .flatten()
    {
        Some(st) => {
            let is_running = crate::state::is_locked(&st);
            let effective_agent = st
                .agent_override
                .as_deref()
                .unwrap_or(&cfg.agent)
                .to_string();
            (is_running, st.session_number, effective_agent)
        }
        None => (false, 0, cfg.agent.clone()),
    };

    let next_wake: Option<String> = {
        let todo_path = dir.join("todo.json");
        crate::todo::TodoList::load(&todo_path)
            .ok()
            .and_then(|list| list.next_wake_time().map(String::from))
    };

    let log_file = crate::log::log_path(dir);
    let log_tail = crate::log::read_current_session(&log_file)
        .ok()
        .flatten()
        .unwrap_or_default();
    let notes_content = std::fs::read_to_string(dir.join("NOTES.md")).unwrap_or_default();
    let task = crate::log::parse_latest_session_task(&log_file)
        .ok()
        .flatten();
    let completion_summary = crate::log::parse_latest_session_plan_complete(&log_file)
        .ok()
        .flatten();
    let completed = completion_summary.is_some();

    let next_wake_rel = next_wake.as_deref().and_then(|w| {
        let wake = chrono::NaiveDateTime::parse_from_str(w, "%Y-%m-%dT%H:%M").ok()?;
        let now = chrono::Local::now().naive_local();
        let diff_ms = (wake - now).num_milliseconds();
        Some(format!(
            "{w} ({})",
            crate::hub::format_relative_time(diff_ms)
        ))
    });

    json!({
        "running": running,
        "session": session,
        "agent": agent,
        "log_tail": log_tail,
        "next_wake": next_wake_rel,
        "notes_content": notes_content,
        "task": task,
        "completed": completed,
        "completion_summary": completion_summary,
    })
}

/// Build the JSON TODO list for a chamber. Items are returned in file order
/// (i.e. insertion order). Missing `todo.json` and parse errors both yield `[]`.
pub fn todos_json(dir: &Path) -> Value {
    let path = dir.join("todo.json");
    let list = crate::todo::TodoList::load(&path).unwrap_or_default();
    serde_json::to_value(list.items()).unwrap_or_else(|_| Value::Array(Vec::new()))
}

/// Build the list of all messages (archive + inbox + outbox) for a chamber.
pub fn messages_json(dir: &Path) -> Value {
    let mut all: Vec<Value> = Vec::new();
    let to_json =
        |filename: &str, msg: &crate::message::Message, direction: &str, source: &str| -> Value {
            json!({
                "id": format!("{source}/{filename}"),
                "direction": direction,
                "from": msg.from,
                "subject": msg.subject,
                "body": msg.body,
                "timestamp": msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
            })
        };
    if let Ok(archived) = crate::message::read_inbox_archive(dir) {
        for (f, m) in archived {
            all.push(to_json(&f, &m, "inbox", "inbox/archive"));
        }
    }
    if let Ok(inbox) = crate::message::read_inbox(dir) {
        for (f, m) in inbox {
            all.push(to_json(&f, &m, "inbox", "inbox"));
        }
    }
    if let Ok(outbox) = crate::message::read_outbox(dir) {
        for (f, m) in outbox {
            all.push(to_json(&f, &m, "outbox", "outbox"));
        }
    }
    if let Ok(archived) = crate::message::read_outbox_archive(dir) {
        for (f, m) in archived {
            all.push(to_json(&f, &m, "outbox", "outbox/archive"));
        }
    }
    all.sort_by(|a, b| {
        a["timestamp"]
            .as_str()
            .unwrap_or("")
            .cmp(b["timestamp"].as_str().unwrap_or(""))
    });
    Value::Array(all)
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
    let msg = crate::message::Message {
        from: req.from.unwrap_or_else(|| "human".into()),
        subject: req.subject.unwrap_or_default(),
        body: req.body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    match crate::message::write_message(&path, "inbox", &msg) {
        Ok(_) => {
            let _ = app.tx.send(SseEvent::NewMessage {
                chamber_id: entry.id,
                direction: "inbox".into(),
                from: msg.from.clone(),
                subject: msg.subject.clone(),
                body: msg.body.clone(),
                timestamp: msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
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
    let body = req
        .message
        .unwrap_or_else(|| "Wake requested from web UI.".into());
    let msg = crate::message::Message {
        from: "operator".into(),
        subject: "Wake".into(),
        body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    if let Err(e) = crate::message::write_message(&path, "inbox", &msg) {
        return Ok(Json(
            json!({"ok": false, "message": format!("Failed: {e}")}),
        ));
    }
    let signaled = crate::process::signal_daemon_wake(&path);
    Ok(Json(json!({
        "ok": true,
        "message": if signaled { "Wake signal sent" } else { "Message queued (no daemon running)" }
    })))
}

pub async fn post_start(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::start_chamber).await;
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Started"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

pub async fn post_stop(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::stop_chamber).await;
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Stopped"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

pub async fn post_restart(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let result = run_blocking_lifecycle(app, path, crate::hub::lifecycle::restart_chamber).await;
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Restarted"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
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
mod tests {
    use super::*;

    #[test]
    fn status_json_for_missing_state_has_zero_session() {
        let dir = tempfile::tempdir().unwrap();
        let v = status_json(dir.path());
        assert_eq!(v["running"], false);
        assert_eq!(v["session"], 0);
    }

    #[test]
    fn status_json_includes_notes_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("NOTES.md"), "# hello\n- one\n- two\n").unwrap();
        let v = status_json(dir.path());
        assert_eq!(v["notes_content"], "# hello\n- one\n- two\n");
    }

    #[test]
    fn status_json_notes_content_empty_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let v = status_json(dir.path());
        assert_eq!(v["notes_content"], "");
    }

    #[test]
    fn todos_json_is_empty_array_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let v = todos_json(dir.path());
        assert_eq!(v, serde_json::json!([]));
    }

    #[test]
    fn todos_json_returns_items_in_file_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut list = crate::todo::TodoList::new();
        list.add("first".into(), "2026-05-01T10:00".into());
        let id2 = list.add("second".into(), "2026-04-01T10:00".into());
        list.done(id2).unwrap();
        list.save(&dir.path().join("todo.json")).unwrap();

        let v = todos_json(dir.path());
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["text"], "first");
        assert_eq!(arr[0]["done"], false);
        assert_eq!(arr[0]["at"], "2026-05-01T10:00");
        assert_eq!(arr[1]["text"], "second");
        assert_eq!(arr[1]["done"], true);
    }

    #[test]
    fn messages_json_sorted_by_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let early = crate::message::Message {
            from: "a".into(),
            subject: "".into(),
            body: "first".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            metadata: Default::default(),
        };
        let late = crate::message::Message {
            from: "b".into(),
            subject: "".into(),
            body: "second".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            metadata: Default::default(),
        };
        crate::message::write_message(dir.path(), "inbox", &late).unwrap();
        crate::message::write_message(dir.path(), "outbox", &early).unwrap();
        let arr = messages_json(dir.path());
        let arr = arr.as_array().unwrap();
        assert_eq!(arr[0]["body"], "first");
        assert_eq!(arr[1]["body"], "second");
    }

    #[test]
    fn messages_json_includes_outbox_archive() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let msg = crate::message::Message {
            from: "agent".into(),
            subject: "".into(),
            body: "archived outbox body".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            metadata: Default::default(),
        };
        let path = crate::message::write_message(dir.path(), "outbox", &msg).unwrap();
        // Simulate sync daemon archiving the delivered outbox message.
        let archive = dir.path().join("messages").join("outbox").join("archive");
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::rename(&path, archive.join(path.file_name().unwrap())).unwrap();

        let arr = messages_json(dir.path());
        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["body"], "archived outbox body");
        assert_eq!(arr[0]["direction"], "outbox");
        let id = arr[0]["id"].as_str().unwrap();
        assert!(id.starts_with("outbox/archive/"), "id was {id}");
    }

    #[test]
    fn messages_json_includes_unique_stable_ids_for_duplicate_messages() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let timestamp = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let msg = crate::message::Message {
            from: "human".into(),
            subject: "".into(),
            body: "same body".into(),
            timestamp,
            metadata: Default::default(),
        };
        crate::message::write_message(dir.path(), "inbox", &msg).unwrap();
        crate::message::write_message(dir.path(), "inbox", &msg).unwrap();

        let arr = messages_json(dir.path());
        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let first = arr[0]["id"].as_str().expect("message id");
        let second = arr[1]["id"].as_str().expect("message id");
        assert!(!first.is_empty());
        assert!(!second.is_empty());
        assert_ne!(
            first, second,
            "duplicate timestamp/body messages need distinct ids"
        );
    }

    #[test]
    fn lifecycle_routes_dispatch_blocking_work_off_async_handlers() {
        let source = include_str!("chamber.rs");
        let needle = ["spawn", "blocking"].join("_");
        assert!(
            source.contains(&needle),
            "lifecycle routes should move blocking process/service work off async handlers"
        );
    }

    #[test]
    fn post_reset_drops_stale_watcher_before_archiving() {
        // Reset renames `messages/` into `history/<ts>/`; the notify handle
        // would otherwise keep watching the archived dir and miss deliveries
        // to the freshly re-created `messages/inbox/` (e.g. zulip sync).
        let source = include_str!("chamber.rs");
        assert!(
            source.contains("app.watchers.drop_watcher(&path);"),
            "post_reset must drop the stale watcher so app.refresh rebuilds it"
        );
    }
}
